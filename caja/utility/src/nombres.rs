//! Los objetos con nombre de utility.library (V39): `AllocNamedObjectA`, `AddNamedObject`, `FindNamedObject`,
//! `ReleaseNamedObject`, `RemNamedObject`, `AttemptRemNamedObject`, `FreeNamedObject` y `NamedObjectName` (autodocs de
//! utility.library; `utility/name.h`). Los usa datatypes.library para su lista de tipos.
//!
//! De un objeto, el programa solo ve `no_Object` (el primer LONG del bloque, `struct NamedObject`); detrás va una copia
//! de su nombre y, si la pidió (`ANO_UserSpace`), su memoria de usuario, alineada a 4. Lo demás (su espacio de nombres,
//! si tiene; en cuál está; su prioridad; cuántos lo usan; quién espera a que se suelte) lo lleva utility aquí. El
//! espacio raíz es el de `nameSpace` = 0.
//!
//! Lo hostil: un objeto que utility no dio (o ya soltado, o de una tarea que acabó) es un aviso y "no se pudo"; un
//! objeto no entra en dos espacios ni en el suyo propio; no se suelta uno que está en un espacio, que alguien más usa
//! o cuyo espacio no está vacío.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Fallo, Memoria};

use crate::{l8, minuscula};

/// Etiquetas de `AllocNamedObjectA`.
pub const ANO_NAMESPACE: u32 = 4000;
pub const ANO_USERSPACE: u32 = 4001;
pub const ANO_PRIORITY: u32 = 4002;
pub const ANO_FLAGS: u32 = 4003;
/// Opciones de un espacio de nombres.
pub const NSF_NODUPS: u32 = 1;
pub const NSF_CASE: u32 = 2;

/// Tope de un nombre (un objeto con un nombre sin fin no se crea).
const MAX_NOMBRE: u32 = 1024;
/// Tope de memoria de usuario.
const MAX_USUARIO: u32 = 16 << 20;
/// Tope de objetos vivos.
const MAX_OBJETOS: usize = 4096;

/// Un objeto con nombre.
#[derive(Clone, Debug)]
pub(crate) struct Objeto {
    nombre: Vec<u8>,
    /// Bytes del bloque (lo que se suelta).
    largo: u32,
    /// Dueño del bloque al reservarlo (si cambia, el bloque ya no es suyo: la tarea acabó).
    dueno: u32,
    prioridad: i8,
    /// Sus opciones de espacio de nombres, si tiene uno (`ANO_NameSpace`).
    espacio: Option<u32>,
    /// En qué espacio está (`Some(0)`: el raíz).
    en: Option<u32>,
    usos: u32,
    /// `RemNamedObject` pendiente: el mensaje que contestar al soltarse.
    quitar: Option<u32>,
}

#[derive(Default)]
pub(crate) struct Nombres {
    objetos: BTreeMap<u32, Objeto>,
    /// Los miembros de cada espacio (0: el raíz), por prioridad (como `Enqueue`).
    espacios: BTreeMap<u32, Vec<u32>>,
}

fn igual(a: &[u8], b: &[u8], mayusculas: bool) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| if mayusculas { x == y } else { minuscula(x) == minuscula(y) })
}

fn cadena<M: Memoria>(ex: &Exec<M>, d: u32) -> Result<Vec<u8>, Fallo> {
    if d == 0 {
        return Err(Fallo::PunteroNulo);
    }
    let mut v = Vec::new();
    loop {
        let c = l8(ex, d.checked_add(v.len() as u32).ok_or(Fallo::FueraDeMemoria { dir: d, largo: 0 })?)?;
        if c == 0 {
            return Ok(v);
        }
        if v.len() as u32 >= MAX_NOMBRE {
            return Err(Fallo::CadenaSinFin { dir: d });
        }
        v.push(c);
    }
}

impl Nombres {
    /// Quita lo de los objetos cuyo bloque ya no es suyo (soltado por la caja al acabar su tarea).
    pub(crate) fn podar<M: Memoria>(&mut self, ex: &Exec<M>) {
        let muertos: Vec<u32> = self
            .objetos
            .iter()
            .filter(|(d, o)| !ex.bloque(**d).is_some_and(|b| b.dueno == o.dueno && b.largo >= o.largo))
            .map(|(d, _)| *d)
            .collect();
        for d in muertos {
            self.objetos.remove(&d);
            self.espacios.remove(&d);
            for v in self.espacios.values_mut() {
                v.retain(|&x| x != d);
            }
        }
        for o in self.objetos.values_mut() {
            if o.en.is_some_and(|e| e != 0 && !self.espacios.contains_key(&e)) {
                o.en = None;
            }
        }
    }

    fn objeto(&self, d: u32) -> Result<&Objeto, Fallo> {
        self.objetos.get(&d).ok_or(Fallo::NoReservado { dir: d })
    }

    /// El espacio `ns` (0: el raíz) y sus opciones.
    fn espacio(&self, ns: u32) -> Result<u32, Fallo> {
        if ns == 0 {
            return Ok(0);
        }
        self.objeto(ns)?.espacio.ok_or(Fallo::NoReservado { dir: ns })
    }

    /// `AllocNamedObjectA(name, tags)`.
    pub(crate) fn alloc<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        nombre: u32,
        etiquetas: &[(u32, u32)],
    ) -> Result<u32, Fallo> {
        let n = cadena(ex, nombre)?;
        if self.objetos.len() >= MAX_OBJETOS {
            return Ok(0);
        }
        let dato = |t: u32, defecto: u32| etiquetas.iter().find(|e| e.0 == t).map(|e| e.1).unwrap_or(defecto);
        let espacio = (dato(ANO_NAMESPACE, 0) != 0).then_some(dato(ANO_FLAGS, 0) & (NSF_NODUPS | NSF_CASE));
        let usuario = dato(ANO_USERSPACE, 0);
        if usuario > MAX_USUARIO {
            return Ok(0);
        }
        // no_Object, el nombre con su 0 y, alineada a 4, la memoria de usuario.
        let tras_nombre = (4 + n.len() as u32 + 1 + 3) & !3;
        let largo = tras_nombre + usuario;
        let d = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
        if d == 0 {
            return Ok(0);
        }
        ex.mem.leer_mut(d + 4, n.len() as u32)?.copy_from_slice(&n);
        ex.mem.escribir32(d, if usuario != 0 { d + tras_nombre } else { 0 })?;
        let dueno = ex.bloque(d).map(|b| b.dueno).unwrap_or(0);
        let prioridad = dato(ANO_PRIORITY, 0) as u8 as i8;
        self.objetos.insert(d, Objeto { nombre: n, largo, dueno, prioridad, espacio, en: None, usos: 1, quitar: None });
        if espacio.is_some() {
            self.espacios.insert(d, Vec::new());
        }
        Ok(d)
    }

    /// `AddNamedObject(nameSpace, object)`: TRUE si entra.
    pub(crate) fn add(&mut self, ns: u32, obj: u32) -> Result<bool, Fallo> {
        if obj == 0 {
            return Ok(false);
        }
        let opciones = self.espacio(ns)?;
        let o = self.objeto(obj)?;
        // Ya en uno, en el suyo propio, o en uno que está dentro de él (un círculo).
        if o.en.is_some() || ns == obj || self.contiene(obj, ns) {
            return Err(Fallo::Sospechoso { dir: obj });
        }
        let miembros = self.espacios.get(&ns).cloned().unwrap_or_default();
        if opciones & NSF_NODUPS != 0
            && miembros.iter().any(|m| igual(&self.objetos[m].nombre, &o.nombre, opciones & NSF_CASE != 0))
        {
            return Ok(false);
        }
        // Por prioridad, detrás de los de su misma prioridad (como Enqueue).
        let p = o.prioridad;
        let k = miembros.iter().position(|m| self.objetos[m].prioridad < p).unwrap_or(miembros.len());
        self.espacios.entry(ns).or_default().insert(k, obj);
        if let Some(o) = self.objetos.get_mut(&obj) {
            o.en = Some(ns);
        }
        Ok(true)
    }

    /// Si el espacio de `obj` contiene (a cualquier profundidad) a `ns`.
    fn contiene(&self, obj: u32, ns: u32) -> bool {
        let mut actual = ns;
        for _ in 0..MAX_OBJETOS {
            match self.objetos.get(&actual).and_then(|o| o.en) {
                Some(e) if e == obj => return true,
                Some(0) | None => return false,
                Some(e) => actual = e,
            }
        }
        true
    }

    /// `FindNamedObject(nameSpace, name, lastObject)`: el siguiente que se llame así (`name` 0: cualquiera), con un uso
    /// más.
    pub(crate) fn find<M: Memoria>(&mut self, ex: &Exec<M>, ns: u32, nombre: u32, ultimo: u32) -> Result<u32, Fallo> {
        let opciones = self.espacio(ns)?;
        let n = if nombre == 0 { None } else { Some(cadena(ex, nombre)?) };
        let miembros = self.espacios.get(&ns).cloned().unwrap_or_default();
        let desde = if ultimo == 0 {
            0
        } else {
            match miembros.iter().position(|&m| m == ultimo) {
                Some(k) => k + 1,
                None => return Err(Fallo::NoReservado { dir: ultimo }),
            }
        };
        let hallado = miembros[desde..].iter().copied().find(|m| {
            let o = &self.objetos[m];
            o.quitar.is_none() && n.as_ref().is_none_or(|n| igual(&o.nombre, n, opciones & NSF_CASE != 0))
        });
        if let Some(h) = hallado
            && let Some(o) = self.objetos.get_mut(&h)
        {
            o.usos = o.usos.saturating_add(1);
        }
        Ok(hallado.unwrap_or(0))
    }

    /// Lo saca de su espacio.
    fn sacar(&mut self, obj: u32) {
        if let Some(e) = self.objetos.get_mut(&obj).and_then(|o| o.en.take())
            && let Some(v) = self.espacios.get_mut(&e)
        {
            v.retain(|&x| x != obj);
        }
    }

    /// `ReleaseNamedObject(object)`: un uso menos; si se estaba quitando y ya nadie lo usa, fuera, y el mensaje de
    /// `RemNamedObject` (que se devuelve para contestarlo).
    pub(crate) fn release(&mut self, obj: u32) -> Result<Option<u32>, Fallo> {
        if obj == 0 {
            return Ok(None);
        }
        let o = self.objetos.get_mut(&obj).ok_or(Fallo::NoReservado { dir: obj })?;
        o.usos = o.usos.saturating_sub(1);
        if o.usos == 0
            && let Some(m) = o.quitar.take()
        {
            self.sacar(obj);
            return Ok(Some(m));
        }
        Ok(None)
    }

    /// `RemNamedObject(object, message)`: pide quitarlo (lo hace al soltarse) y lo suelta una vez.
    pub(crate) fn rem(&mut self, obj: u32, mensaje: u32) -> Result<Option<u32>, Fallo> {
        if mensaje == 0 {
            return Err(Fallo::PunteroNulo);
        }
        let o = self.objetos.get_mut(&obj).ok_or(Fallo::NoReservado { dir: obj })?;
        if o.quitar.is_some() {
            return Err(Fallo::Sospechoso { dir: obj });
        }
        o.quitar = Some(mensaje);
        self.release(obj)
    }

    /// `AttemptRemNamedObject(object)`: TRUE si se quita ya (nadie más lo usa ni se está quitando).
    pub(crate) fn attempt_rem(&mut self, obj: u32) -> Result<bool, Fallo> {
        let o = self.objeto(obj)?;
        if o.quitar.is_some() || o.usos > 1 {
            return Ok(false);
        }
        self.sacar(obj);
        if let Some(o) = self.objetos.get_mut(&obj) {
            o.usos = 0;
        }
        Ok(true)
    }

    /// `FreeNamedObject(object)`: suelta su bloque si no está en ningún espacio, nadie más lo usa y su espacio (si lo
    /// tiene) está vacío.
    pub(crate) fn free<M: Memoria>(&mut self, ex: &mut Exec<M>, obj: u32) -> Result<(), Fallo> {
        if obj == 0 {
            return Ok(());
        }
        let o = self.objeto(obj)?;
        let vacio = self.espacios.get(&obj).is_none_or(|v| v.is_empty());
        if o.en.is_some() || o.usos > 1 || !vacio {
            return Err(Fallo::Sospechoso { dir: obj });
        }
        let largo = o.largo;
        ex.free_mem(obj, largo)?;
        self.objetos.remove(&obj);
        self.espacios.remove(&obj);
        Ok(())
    }

    /// `NamedObjectName(object)`: su nombre (la copia del bloque), o 0.
    pub(crate) fn name(&self, obj: u32) -> Result<u32, Fallo> {
        if obj == 0 {
            return Ok(0);
        }
        self.objeto(obj)?;
        Ok(obj + 4)
    }
}
