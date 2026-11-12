//! utility.library nativa de la caja Amiga (S4), versión 40.
//!
//! La abren las bibliotecas de C del Amiga al arrancar un programa (libnix: sus multiplicaciones y divisiones de 32
//! bits en un 68000 van por `SMult32` y `UDivMod32`) y la usan los programas para las etiquetas (`TagItem`), los
//! textos sin distinguir mayúsculas y las fechas. Escrita desde los autodocs de utility.library, `utility_lib.fd` y
//! los includes `utility/tagitem.h` y `utility/date.h` del NDK 3.1. No se ha leído código de AmigaOS ni de AROS.
//!
//! Como las demás bibliotecas nativas: cada entrada de la tabla de saltos es `$A0FF n RTS` (exec la crea con
//! `crear_biblioteca_nativa`), todo puntero del 68k se comprueba (el Enforcer de S3) y un fallo da un aviso y el
//! valor de "no se pudo", nunca un pánico. Las listas de etiquetas que reparte (`AllocateTagItems`,
//! `CloneTagItems`) las apunta en una tabla nativa: `FreeTagItems` de otra cosa da un aviso.
//!
//! Lo que falta: `CallHookPkt` (ejecutar 68k desde una función nativa, como `SetFunction` en exec), los objetos con
//! nombre (V39) y `MapTags`/`FilterTagChanges`/`PackStructureTags`/`UnpackStructureTags`: dan un aviso y 0.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Accion, Aviso, Exec, Fallo, LIMITE, Memoria, PAGINA_CERO, Registros};

/// Las funciones de utility.library 40, desde la 5 (-30), en el orden de `utility_lib.fd`. Las privadas y
/// reservadas van con su sitio.
pub const FUNCIONES: [&str; 41] = [
    "FindTagItem",
    "GetTagData",
    "PackBoolTags",
    "NextTagItem",
    "FilterTagChanges",
    "MapTags",
    "AllocateTagItems",
    "CloneTagItems",
    "FreeTagItems",
    "RefreshTagItemClones",
    "TagInArray",
    "FilterTagItems",
    "CallHookPkt",
    "utilityPrivate1",
    "utilityPrivate2",
    "Amiga2Date",
    "Date2Amiga",
    "CheckDate",
    "SMult32",
    "UMult32",
    "SDivMod32",
    "UDivMod32",
    "Stricmp",
    "Strnicmp",
    "ToUpper",
    "ToLower",
    "ApplyTagChanges",
    "utilityPrivate3",
    "SMult64",
    "UMult64",
    "PackStructureTags",
    "UnpackStructureTags",
    "AddNamedObject",
    "AllocNamedObjectA",
    "AttemptRemNamedObject",
    "FindNamedObject",
    "FreeNamedObject",
    "NamedObjectName",
    "ReleaseNamedObject",
    "RemNamedObject",
    "GetUniqueID",
];

/// La entrada de la tabla de saltos de una función (5 es -30).
pub fn entrada(nombre: &str) -> Option<u16> {
    FUNCIONES.iter().position(|f| *f == nombre).map(|i| i as u16 + 5)
}

/// Etiquetas del sistema (`utility/tagitem.h`).
pub mod tag {
    pub const DONE: u32 = 0;
    pub const IGNORE: u32 = 1;
    pub const MORE: u32 = 2;
    pub const SKIP: u32 = 3;
    pub const USER: u32 = 1 << 31;
    /// `struct TagItem`: ti_Tag y ti_Data.
    pub const TAMANO: u32 = 8;
    /// `FilterTagItems`: deja las del filtro, o quita las del filtro.
    pub const FILTER_AND: u32 = 0;
    pub const FILTER_NOT: u32 = 1;
}

/// `struct ClockData` (`utility/date.h`): siete UWORD.
pub mod clockdata {
    pub const SEC: u32 = 0;
    pub const MIN: u32 = 2;
    pub const HOUR: u32 = 4;
    pub const MDAY: u32 = 6;
    pub const MONTH: u32 = 8;
    pub const YEAR: u32 = 10;
    pub const WDAY: u32 = 12;
    pub const TAMANO: u32 = 14;
}

/// Tope de vueltas al recorrer etiquetas: más es una lista en círculo (un `TAG_MORE` que vuelve atrás).
const MAX_VUELTAS: u32 = 1 << 16;
const MAX_AVISOS: usize = 1024;

/// utility.library de una caja.
pub struct Utilidad {
    base: u32,
    /// Número nativo de la primera función (FindTagItem); los demás siguen.
    primero: u16,
    avisos: Vec<Aviso>,
    /// Listas de etiquetas que dio `AllocateTagItems` o `CloneTagItems`: dirección → tamaño.
    listas: BTreeMap<u32, u32>,
    unico: u32,
}

fn comprobar(dir: u32, largo: u32, escribir: bool) -> Result<(), Fallo> {
    if dir >= LIMITE || largo > LIMITE - dir {
        return Err(Fallo::FueraDeMemoria { dir, largo });
    }
    if dir < PAGINA_CERO && !(dir == 4 && largo == 4 && !escribir) {
        return Err(Fallo::PaginaCero { dir });
    }
    Ok(())
}

fn l8<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u8, Fallo> {
    comprobar(d, 1, false)?;
    ex.mem.leer8(d)
}
fn l16<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u16, Fallo> {
    comprobar(d, 2, false)?;
    ex.mem.leer16(d)
}
fn l32<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<u32, Fallo> {
    comprobar(d, 4, false)?;
    ex.mem.leer32(d)
}
fn e16<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u16) -> Result<(), Fallo> {
    comprobar(d, 2, true)?;
    ex.mem.escribir16(d, v)
}
fn e32<M: Memoria>(ex: &mut Exec<M>, d: u32, v: u32) -> Result<(), Fallo> {
    comprobar(d, 4, true)?;
    ex.mem.escribir32(d, v)
}

/// `ToUpper` de ISO-8859-1 (la tabla de utility sin locale): a-z y à-þ, salvo ÷; ß y ÿ no tienen mayúscula.
pub fn mayuscula(c: u8) -> u8 {
    match c {
        b'a'..=b'z' => c - 32,
        0xE0..=0xFE if c != 0xF7 => c - 32,
        _ => c,
    }
}

/// `ToLower` de ISO-8859-1.
pub fn minuscula(c: u8) -> u8 {
    match c {
        b'A'..=b'Z' => c + 32,
        0xC0..=0xDE if c != 0xD7 => c + 32,
        _ => c,
    }
}

// --- Fechas: segundos desde el 1 de enero de 1978 (un domingo) ---

fn bisiesto(a: u32) -> bool {
    (a.is_multiple_of(4) && !a.is_multiple_of(100)) || a.is_multiple_of(400)
}

fn dias_del_mes(mes: u32, a: u32) -> u32 {
    match mes {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if bisiesto(a) => 29,
        _ => 28,
    }
}

/// Segundos → (seg, min, hora, día, mes, año, día de la semana).
pub fn amiga_a_fecha(segundos: u32) -> [u32; 7] {
    let dias = segundos / 86400;
    let resto = segundos % 86400;
    let mut a = 1978;
    let mut d = dias;
    loop {
        let n = if bisiesto(a) { 366 } else { 365 };
        if d < n {
            break;
        }
        d -= n;
        a += 1;
    }
    let mut mes = 1;
    while d >= dias_del_mes(mes, a) {
        d -= dias_del_mes(mes, a);
        mes += 1;
    }
    [resto % 60, resto / 60 % 60, resto / 3600, d + 1, mes, a, dias % 7]
}

/// (seg, min, hora, día, mes, año) → segundos, si la fecha es válida y cabe en 32 bits desde 1978.
pub fn fecha_a_amiga(f: [u32; 6]) -> Option<u32> {
    let [s, mi, h, dia, mes, a] = f;
    if s > 59 || mi > 59 || h > 23 || !(1..=12).contains(&mes) || !(1978..=2114).contains(&a) {
        return None;
    }
    if dia == 0 || dia > dias_del_mes(mes, a) {
        return None;
    }
    let mut dias: u64 = 0;
    for x in 1978..a {
        dias += if bisiesto(x) { 366 } else { 365 };
    }
    for m in 1..mes {
        dias += dias_del_mes(m, a) as u64;
    }
    dias += (dia - 1) as u64;
    let total = dias * 86400 + h as u64 * 3600 + mi as u64 * 60 + s as u64;
    u32::try_from(total).ok()
}

impl Utilidad {
    /// Crea utility.library en la caja, con sus números nativos desde `primero` (tienen que estar libres).
    pub fn instalar<M: Memoria>(ex: &mut Exec<M>, primero: u16) -> Result<Utilidad, Fallo> {
        let numeros: Vec<u16> = (0..FUNCIONES.len() as u16).map(|i| primero + i).collect();
        // `struct UtilityBase` (V39): la biblioteca, ub_Language y ub_Reserved.
        let base = ex.crear_biblioteca_nativa("utility.library", 40, 1, &numeros, 36)?;
        Ok(Utilidad { base, primero, avisos: Vec::new(), listas: BTreeMap::new(), unico: 0 })
    }

    pub fn base(&self) -> u32 {
        self.base
    }

    /// Si el número nativo es de utility.
    pub fn es_mia(&self, n: u16) -> bool {
        n >= self.primero && n < self.primero + FUNCIONES.len() as u16
    }

    pub fn avisos(&self) -> &[Aviso] {
        &self.avisos
    }

    pub fn tomar_avisos(&mut self) -> Vec<Aviso> {
        core::mem::take(&mut self.avisos)
    }

    fn avisar<M: Memoria>(&mut self, ex: &Exec<M>, funcion: &'static str, fallo: Fallo) {
        if self.avisos.len() >= MAX_AVISOS {
            self.avisos.remove(0);
        }
        self.avisos.push(Aviso { tarea: ex.tarea_actual(), funcion, fallo });
    }

    /// La función nativa `n` con los registros de la tarea. `Accion::Ajena` si no es de utility.
    pub fn llamar<M: Memoria>(&mut self, ex: &mut Exec<M>, n: u16, r: &mut Registros) -> Accion {
        if !self.es_mia(n) {
            return Accion::Ajena;
        }
        let i = (n - self.primero) as usize;
        match self.ejecutar(ex, i, r) {
            Ok(()) => {}
            Err(f) => {
                self.avisar(ex, FUNCIONES[i], f);
                r.d[0] = 0;
            }
        }
        Accion::Seguir
    }

    fn ejecutar<M: Memoria>(&mut self, ex: &mut Exec<M>, i: usize, r: &mut Registros) -> Result<(), Fallo> {
        let (d, a) = (r.d, r.a);
        let v = match FUNCIONES[i] {
            "FindTagItem" => self.find_tag_item(ex, d[0], a[0])?,
            "GetTagData" => {
                let t = self.find_tag_item(ex, d[0], a[0])?;
                if t == 0 { d[1] } else { l32(ex, t + 4)? }
            }
            "PackBoolTags" => self.pack_bool_tags(ex, d[0], a[0], a[1])?,
            "NextTagItem" => {
                if a[0] == 0 {
                    return Err(Fallo::PunteroNulo);
                }
                let p = l32(ex, a[0])?;
                let (t, sig) = self.siguiente(ex, p)?;
                e32(ex, a[0], sig)?;
                t
            }
            "AllocateTagItems" => self.allocate_tag_items(ex, d[0])?,
            "CloneTagItems" => self.clone_tag_items(ex, a[0])?,
            "FreeTagItems" => {
                self.free_tag_items(ex, a[0])?;
                0
            }
            "RefreshTagItemClones" => {
                self.refresh_tag_item_clones(ex, a[0], a[1])?;
                0
            }
            "TagInArray" => self.tag_in_array(ex, d[0], a[0])? as u32,
            "FilterTagItems" => self.filter_tag_items(ex, a[0], a[1], d[0])?,
            "ApplyTagChanges" => {
                self.apply_tag_changes(ex, a[0], a[1])?;
                0
            }
            "Amiga2Date" => {
                comprobar(a[0], clockdata::TAMANO, true)?;
                ex.mem.leer(a[0], clockdata::TAMANO)?;
                let f = amiga_a_fecha(d[0]);
                for (k, campo) in [
                    clockdata::SEC,
                    clockdata::MIN,
                    clockdata::HOUR,
                    clockdata::MDAY,
                    clockdata::MONTH,
                    clockdata::YEAR,
                    clockdata::WDAY,
                ]
                .into_iter()
                .enumerate()
                {
                    e16(ex, a[0] + campo, f[k] as u16)?;
                }
                0
            }
            "Date2Amiga" | "CheckDate" => {
                let f = self.leer_fecha(ex, a[0])?;
                match fecha_a_amiga(f) {
                    Some(s) => s,
                    None if FUNCIONES[i] == "CheckDate" => 0,
                    // Date2Amiga no comprueba: con una fecha imposible, 0 y un aviso.
                    None => return Err(Fallo::Sospechoso { dir: a[0] }),
                }
            }
            "SMult32" => (d[0] as i32).wrapping_mul(d[1] as i32) as u32,
            "UMult32" => d[0].wrapping_mul(d[1]),
            "SDivMod32" => {
                if d[1] == 0 {
                    return Err(Fallo::Sospechoso { dir: 0 });
                }
                let (x, y) = (d[0] as i32, d[1] as i32);
                r.d[1] = x.wrapping_rem(y) as u32;
                x.wrapping_div(y) as u32
            }
            "UDivMod32" => {
                if d[1] == 0 {
                    return Err(Fallo::Sospechoso { dir: 0 });
                }
                r.d[1] = d[0] % d[1];
                d[0] / d[1]
            }
            "SMult64" => {
                let p = (d[0] as i32 as i64).wrapping_mul(d[1] as i32 as i64) as u64;
                r.d[1] = p as u32;
                (p >> 32) as u32
            }
            "UMult64" => {
                let p = d[0] as u64 * d[1] as u64;
                r.d[1] = p as u32;
                (p >> 32) as u32
            }
            "Stricmp" => self.comparar(ex, a[0], a[1], u32::MAX)? as u32,
            "Strnicmp" => self.comparar(ex, a[0], a[1], d[0])? as u32,
            "ToUpper" => mayuscula(d[0] as u8) as u32,
            "ToLower" => minuscula(d[0] as u8) as u32,
            "GetUniqueID" => {
                self.unico = self.unico.wrapping_add(1);
                self.unico
            }
            _ => return Err(Fallo::NoImplementada { n: self.primero + i as u16 }),
        };
        r.d[0] = v;
        Ok(())
    }

    fn leer_fecha<M: Memoria>(&self, ex: &Exec<M>, cd: u32) -> Result<[u32; 6], Fallo> {
        if cd == 0 {
            return Err(Fallo::PunteroNulo);
        }
        let mut f = [0; 6];
        for (k, campo) in
            [clockdata::SEC, clockdata::MIN, clockdata::HOUR, clockdata::MDAY, clockdata::MONTH, clockdata::YEAR]
                .into_iter()
                .enumerate()
        {
            f[k] = l16(ex, cd + campo)? as u32;
        }
        Ok(f)
    }

    /// Lo que hace `NextTagItem` desde `p`: la siguiente etiqueta de usuario y dónde sigue la lista (0 al final).
    fn siguiente<M: Memoria>(&self, ex: &Exec<M>, mut p: u32) -> Result<(u32, u32), Fallo> {
        for _ in 0..MAX_VUELTAS {
            if p == 0 {
                return Ok((0, 0));
            }
            let t = l32(ex, p)?;
            let dato = l32(ex, p.checked_add(4).ok_or(Fallo::FueraDeMemoria { dir: p, largo: 8 })?)?;
            match t {
                tag::DONE => return Ok((0, 0)),
                tag::MORE => p = dato,
                tag::IGNORE => p = p.wrapping_add(tag::TAMANO),
                tag::SKIP => p = p.wrapping_add(dato.wrapping_add(1).wrapping_mul(tag::TAMANO)),
                _ => return Ok((p, p.wrapping_add(tag::TAMANO))),
            }
        }
        Err(Fallo::ListaRota { dir: p })
    }

    /// Recorre las etiquetas de usuario de una lista: `f(dirección, etiqueta, dato)`; `false` para parar.
    fn recorrer<M: Memoria>(
        &self,
        ex: &Exec<M>,
        lista: u32,
        mut f: impl FnMut(u32, u32, u32) -> bool,
    ) -> Result<(), Fallo> {
        let mut p = lista;
        for _ in 0..MAX_VUELTAS {
            let (t, sig) = self.siguiente(ex, p)?;
            if t == 0 {
                return Ok(());
            }
            if !f(t, l32(ex, t)?, l32(ex, t + 4)?) {
                return Ok(());
            }
            p = sig;
        }
        Err(Fallo::ListaRota { dir: lista })
    }

    fn find_tag_item<M: Memoria>(&self, ex: &Exec<M>, etiqueta: u32, lista: u32) -> Result<u32, Fallo> {
        let mut hallada = 0;
        self.recorrer(ex, lista, |d, t, _| {
            if t == etiqueta {
                hallada = d;
                return false;
            }
            true
        })?;
        Ok(hallada)
    }

    fn pack_bool_tags<M: Memoria>(&self, ex: &Exec<M>, mut flags: u32, lista: u32, mapa: u32) -> Result<u32, Fallo> {
        let mut pares = Vec::new();
        self.recorrer(ex, lista, |_, t, v| {
            pares.push((t, v));
            true
        })?;
        for (t, v) in pares {
            let m = self.find_tag_item(ex, t, mapa)?;
            if m != 0 {
                let bits = l32(ex, m + 4)?;
                if v != 0 {
                    flags |= bits;
                } else {
                    flags &= !bits;
                }
            }
        }
        Ok(flags)
    }

    fn tag_in_array<M: Memoria>(&self, ex: &Exec<M>, etiqueta: u32, tabla: u32) -> Result<bool, Fallo> {
        if tabla == 0 {
            return Ok(false);
        }
        for k in 0..MAX_VUELTAS {
            let t = l32(ex, tabla.checked_add(k * 4).ok_or(Fallo::ListaRota { dir: tabla })?)?;
            if t == tag::DONE {
                return Ok(false);
            }
            if t == etiqueta {
                return Ok(true);
            }
        }
        Err(Fallo::ListaRota { dir: tabla })
    }

    /// `FilterTagItems`: las que no pasan el filtro pasan a `TAG_IGNORE`. Devuelve cuántas quedan.
    fn filter_tag_items<M: Memoria>(
        &self,
        ex: &mut Exec<M>,
        lista: u32,
        filtro: u32,
        logica: u32,
    ) -> Result<u32, Fallo> {
        if logica > tag::FILTER_NOT {
            return Err(Fallo::Sospechoso { dir: lista });
        }
        let mut todas = Vec::new();
        self.recorrer(ex, lista, |d, t, _| {
            todas.push((d, t));
            true
        })?;
        let mut quedan = 0;
        for (d, t) in todas {
            let esta = self.tag_in_array(ex, t, filtro)?;
            if esta == (logica == tag::FILTER_AND) {
                quedan += 1;
            } else {
                e32(ex, d, tag::IGNORE)?;
            }
        }
        Ok(quedan)
    }

    fn apply_tag_changes<M: Memoria>(&self, ex: &mut Exec<M>, lista: u32, cambios: u32) -> Result<(), Fallo> {
        let mut pares = Vec::new();
        self.recorrer(ex, cambios, |_, t, v| {
            pares.push((t, v));
            true
        })?;
        for (t, v) in pares {
            let d = self.find_tag_item(ex, t, lista)?;
            if d != 0 {
                e32(ex, d + 4, v)?;
            }
        }
        Ok(())
    }

    fn allocate_tag_items<M: Memoria>(&mut self, ex: &mut Exec<M>, n: u32) -> Result<u32, Fallo> {
        // Lo más: lo que cabe en la memoria del 68k.
        if n == 0 || n > (64 << 20) / tag::TAMANO {
            return Ok(0);
        }
        let largo = n * tag::TAMANO;
        let d = ex.alloc_vec(largo, memf::PUBLIC | memf::CLEAR)?;
        if d != 0 {
            self.listas.insert(d, largo);
        }
        Ok(d)
    }

    fn clone_tag_items<M: Memoria>(&mut self, ex: &mut Exec<M>, lista: u32) -> Result<u32, Fallo> {
        let mut pares = Vec::new();
        self.recorrer(ex, lista, |_, t, v| {
            pares.push((t, v));
            true
        })?;
        let d = self.allocate_tag_items(ex, pares.len() as u32 + 1)?;
        if d == 0 {
            return Ok(0);
        }
        for (k, (t, v)) in pares.into_iter().enumerate() {
            e32(ex, d + k as u32 * tag::TAMANO, t)?;
            e32(ex, d + k as u32 * tag::TAMANO + 4, v)?;
        }
        Ok(d)
    }

    fn free_tag_items<M: Memoria>(&mut self, ex: &mut Exec<M>, d: u32) -> Result<(), Fallo> {
        if d == 0 {
            return Ok(());
        }
        if !self.listas.contains_key(&d) {
            return Err(Fallo::NoReservado { dir: d });
        }
        self.listas.remove(&d);
        ex.free_vec(d)
    }

    /// `RefreshTagItemClones`: vuelve a copiar en el clon las etiquetas del original (el clon tiene que ser de
    /// `CloneTagItems` de esa lista).
    fn refresh_tag_item_clones<M: Memoria>(&self, ex: &mut Exec<M>, clon: u32, original: u32) -> Result<(), Fallo> {
        if clon == 0 {
            return Ok(());
        }
        let Some(&largo) = self.listas.get(&clon) else { return Err(Fallo::NoReservado { dir: clon }) };
        let mut pares = Vec::new();
        self.recorrer(ex, original, |_, t, v| {
            pares.push((t, v));
            true
        })?;
        let caben = (largo / tag::TAMANO).saturating_sub(1) as usize;
        for (k, (t, v)) in pares.into_iter().take(caben).enumerate() {
            e32(ex, clon + k as u32 * tag::TAMANO, t)?;
            e32(ex, clon + k as u32 * tag::TAMANO + 4, v)?;
        }
        Ok(())
    }

    /// `Stricmp` y `Strnicmp`: <0, 0 o >0, sin distinguir mayúsculas (Latin-1).
    fn comparar<M: Memoria>(&self, ex: &Exec<M>, a: u32, b: u32, n: u32) -> Result<i32, Fallo> {
        if a == 0 || b == 0 {
            return Err(Fallo::PunteroNulo);
        }
        let tope = n.min(1 << 20);
        for i in 0..tope {
            let x = mayuscula(l8(ex, a.checked_add(i).ok_or(Fallo::CadenaSinFin { dir: a })?)?);
            let y = mayuscula(l8(ex, b.checked_add(i).ok_or(Fallo::CadenaSinFin { dir: b })?)?);
            if x != y {
                return Ok(x as i32 - y as i32);
            }
            if x == 0 {
                return Ok(0);
            }
        }
        if n == u32::MAX { Err(Fallo::CadenaSinFin { dir: a }) } else { Ok(0) }
    }

    /// Olvida las listas de etiquetas de una tarea que muere (exec ya soltó su memoria).
    pub fn tarea_muerta<M: Memoria>(&mut self, ex: &Exec<M>) {
        self.listas.retain(|&d, _| ex.bloque(d.wrapping_sub(4)).is_some_and(|b| b.vec));
    }

    /// Cuántas listas de etiquetas repartidas siguen vivas.
    pub fn listas_vivas(&self) -> usize {
        self.listas.len()
    }
}
