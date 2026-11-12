//! Listas de exec: `Insert`, `AddHead`, `AddTail`, `Remove`, `RemHead`, `RemTail`, `Enqueue`, `FindName` y
//! `NewList`, sobre `List`/`Node` (y `MinList`/`MinNode`, que tienen los enlaces en el mismo sitio) en la memoria
//! del 68k.
//!
//! Una lista vacía apunta a sí misma: `lh_Head` a `&lh_Tail`, `lh_Tail` a 0 y `lh_TailPred` a la lista. Así la
//! cabeza y la cola hacen de nodos centinela y ninguna operación tiene casos aparte (RKRM: Libraries, "Exec
//! Lists and Queues").
//!
//! Antes de escribir se mira que los enlaces que se van a tocar se apunten entre sí. Un exec de verdad no lo mira y
//! una lista rota corrompe la memoria en silencio; aquí da `Fallo::ListaRota` y no se toca nada.

use alloc::vec::Vec;

use crate::estructuras::{list, node};
use crate::{Exec, Fallo, MAX_VUELTAS, Memoria};

const SUCC: u32 = node::LN_SUCC;
const PRED: u32 = node::LN_PRED;

impl<M: Memoria> Exec<M> {
    /// `NewList`: deja la lista vacía y con su tipo.
    pub fn new_list(&mut self, l: u32, tipo: u8) -> Result<(), Fallo> {
        if l == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(l, list::TAMANO - 2)?;
        self.e32(l + list::LH_HEAD, l + list::LH_TAIL)?;
        self.e32(l + list::LH_TAIL, 0)?;
        self.e32(l + list::LH_TAILPRED, l)?;
        if tipo != 0 {
            self.e8(l + list::LH_TYPE, tipo)?;
        }
        Ok(())
    }

    fn comprobar_nodo(&self, n: u32) -> Result<(), Fallo> {
        if n == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(n, 8)
    }

    /// `AddHead`.
    pub fn add_head(&mut self, l: u32, n: u32) -> Result<(), Fallo> {
        self.comprobar_nodo(l)?;
        self.comprobar_nodo(n)?;
        let cabeza = self.lp(l + list::LH_HEAD)?;
        if self.lp(cabeza + PRED)? != l {
            return Err(Fallo::ListaRota { dir: l });
        }
        self.e32(n + SUCC, cabeza)?;
        self.e32(n + PRED, l)?;
        self.e32(cabeza + PRED, n)?;
        self.e32(l + list::LH_HEAD, n)
    }

    /// `AddTail`.
    pub fn add_tail(&mut self, l: u32, n: u32) -> Result<(), Fallo> {
        self.comprobar_nodo(l)?;
        self.comprobar_nodo(n)?;
        let ultimo = self.lp(l + list::LH_TAILPRED)?;
        if self.lp(ultimo + SUCC)? != l + list::LH_TAIL {
            return Err(Fallo::ListaRota { dir: l });
        }
        self.e32(n + SUCC, l + list::LH_TAIL)?;
        self.e32(n + PRED, ultimo)?;
        self.e32(ultimo + SUCC, n)?;
        self.e32(l + list::LH_TAILPRED, n)
    }

    /// `Insert`: pone `n` detrás de `pred`; con `pred` 0, al principio.
    pub fn insert(&mut self, l: u32, n: u32, pred: u32) -> Result<(), Fallo> {
        if pred == 0 {
            return self.add_head(l, n);
        }
        self.comprobar_nodo(n)?;
        self.comprobar_nodo(pred)?;
        let sig = self.lp(pred + SUCC)?;
        if sig == 0 {
            // `pred` es el centinela de la cola: no se puede poner nada detrás.
            return Err(Fallo::ListaRota { dir: pred });
        }
        if self.lp(sig + PRED)? != pred {
            return Err(Fallo::ListaRota { dir: pred });
        }
        self.e32(n + SUCC, sig)?;
        self.e32(n + PRED, pred)?;
        self.e32(sig + PRED, n)?;
        self.e32(pred + SUCC, n)
    }

    /// `Remove`: saca el nodo de la lista en que esté. Como en exec, no toca los enlaces del nodo.
    pub fn remove(&mut self, n: u32) -> Result<(), Fallo> {
        self.comprobar_nodo(n)?;
        let sig = self.lp(n + SUCC)?;
        let ant = self.lp(n + PRED)?;
        let bien = sig != 0 && ant != 0 && self.lp(ant + SUCC)? == n && self.lp(sig + PRED)? == n;
        if !bien {
            return Err(Fallo::ListaRota { dir: n });
        }
        self.e32(ant + SUCC, sig)?;
        self.e32(sig + PRED, ant)
    }

    /// `RemHead`: el primer nodo, sacado, o 0 si la lista está vacía.
    pub fn rem_head(&mut self, l: u32) -> Result<u32, Fallo> {
        self.comprobar_nodo(l)?;
        let n = self.lp(l + list::LH_HEAD)?;
        let sig = self.lp(n + SUCC)?;
        if sig == 0 {
            return Ok(0);
        }
        if self.lp(n + PRED)? != l || self.lp(sig + PRED)? != n {
            return Err(Fallo::ListaRota { dir: l });
        }
        self.e32(l + list::LH_HEAD, sig)?;
        self.e32(sig + PRED, l)?;
        Ok(n)
    }

    /// `RemTail`: el último nodo, sacado, o 0.
    pub fn rem_tail(&mut self, l: u32) -> Result<u32, Fallo> {
        self.comprobar_nodo(l)?;
        let n = self.lp(l + list::LH_TAILPRED)?;
        let ant = self.lp(n + PRED)?;
        if ant == 0 {
            return Ok(0);
        }
        if self.lp(n + SUCC)? != l + list::LH_TAIL || self.lp(ant + SUCC)? != n {
            return Err(Fallo::ListaRota { dir: l });
        }
        self.e32(l + list::LH_TAILPRED, ant)?;
        self.e32(ant + SUCC, l + list::LH_TAIL)?;
        Ok(n)
    }

    /// `Enqueue`: por prioridad (`ln_Pri`, de más a menos); detrás de los de su misma prioridad, así los iguales
    /// van por turnos.
    pub fn enqueue(&mut self, l: u32, n: u32) -> Result<(), Fallo> {
        self.comprobar_nodo(l)?;
        self.tramo(n, node::TAMANO)?;
        let pri = self.l8(n + node::LN_PRI)? as i8;
        let mut p = self.lp(l + list::LH_HEAD)?;
        let mut vueltas = 0;
        loop {
            let sig = self.lp(p + SUCC)?;
            if sig == 0 || (self.l8(p + node::LN_PRI)? as i8) < pri {
                break;
            }
            p = sig;
            vueltas += 1;
            if vueltas > MAX_VUELTAS {
                return Err(Fallo::ListaRota { dir: l });
            }
        }
        // Delante de `p`.
        let ant = self.lp(p + PRED)?;
        if self.lp(ant + SUCC)? != p {
            return Err(Fallo::ListaRota { dir: l });
        }
        self.e32(n + SUCC, p)?;
        self.e32(n + PRED, ant)?;
        self.e32(ant + SUCC, n)?;
        self.e32(p + PRED, n)
    }

    /// `FindName`: el primer nodo después de `inicio` con ese nombre, o 0. `inicio` puede ser la lista o un nodo
    /// (para seguir buscando desde él), como en exec.
    pub fn find_name(&self, inicio: u32, nombre: &[u8]) -> Result<u32, Fallo> {
        self.comprobar_nodo(inicio)?;
        let mut n = self.lp(inicio + SUCC)?;
        let mut vueltas = 0;
        loop {
            let sig = self.lp(n + SUCC)?;
            if sig == 0 {
                return Ok(0);
            }
            let p = self.lp(n + node::LN_NAME)?;
            if p != 0 && self.cadena(p)? == nombre {
                return Ok(n);
            }
            n = sig;
            vueltas += 1;
            if vueltas > MAX_VUELTAS {
                return Err(Fallo::ListaRota { dir: inicio });
            }
        }
    }

    /// Los nodos de una lista, en orden.
    pub fn nodos(&self, l: u32) -> Result<Vec<u32>, Fallo> {
        self.comprobar_nodo(l)?;
        let mut v = Vec::new();
        let mut n = self.lp(l + list::LH_HEAD)?;
        loop {
            let sig = self.lp(n + SUCC)?;
            if sig == 0 {
                return Ok(v);
            }
            v.push(n);
            n = sig;
            if v.len() as u32 > MAX_VUELTAS {
                return Err(Fallo::ListaRota { dir: l });
            }
        }
    }

    /// Si la lista está vacía.
    pub fn lista_vacia(&self, l: u32) -> Result<bool, Fallo> {
        Ok(self.lp(l + list::LH_TAILPRED)? == l)
    }
}
