//! La lista de pantallas públicas (`intuition/screens.h`, autodocs de intuition): `LockPubScreenList` y
//! `UnlockPubScreenList`, `NextPubScreen`, `SetDefaultPubScreen` y `GetDefaultPubScreen`, y `SetPubScreenModes`.
//!
//! - `LockPubScreenList` da una `struct List` de `struct PubScreenNode` (el Workbench primero, luego las propias con
//!   nombre, en el orden en que se abrieron; las que están en modo privado, con `PSNF_PRIVATE`) en la memoria del sistema.
//!   Es una foto: no cambia mientras alguien la tenga, y se suelta con el último `UnlockPubScreenList` (o al acabar la
//!   tarea que la tenía).
//! - La pantalla pública de serie es la que da `LockPubScreen(NULL)`: el Workbench, o la de `SetDefaultPubScreen` mientras
//!   siga abierta y pública.
//! - `SHANGHAI`: las ventanas que se abren en el Workbench sin pedirlo (sin pantalla) van a la de serie; `POPPUBSCREEN`:
//!   una pantalla pública pasa delante al abrirse en ella una ventana.

use alloc::vec::Vec;

use caja_exec::estructuras::{list, memf, node};
use caja_exec::{Exec, Memoria};

use crate::acceso::Acceso;
use crate::{CajaGrafica, Compositor, Fallo};

/// `struct PubScreenNode`.
mod psn {
    pub const SCREEN: u32 = 14;
    pub const FLAGS: u32 = 18;
    pub const SIZE: u32 = 20;
    pub const VISITORCOUNT: u32 = 22;
    pub const SIGTASK: u32 = 24;
    pub const SIGBIT: u32 = 28;
    pub const TAMANO: u32 = 30;
}

/// `psn_Flags`.
const PSNF_PRIVATE: u16 = 1;
/// `SetPubScreenModes`.
pub(crate) const SHANGHAI: u32 = 0x01;
pub(crate) const POPPUBSCREEN: u32 = 0x02;
/// `MAXPUBSCREENNAME`: lo más largo de un nombre (sin el 0 del final).
const MAXPUBSCREENNAME: usize = 139;

/// La foto de la lista que tienen los programas: su `List`, lo reservado (dirección y largo) y quién la tiene (una vez
/// por cada `LockPubScreenList`).
#[derive(Default)]
pub(crate) struct ListaPub {
    pub lista: u32,
    pub bloques: Vec<(u32, u32)>,
    pub tareas: Vec<u32>,
}

impl<C: Compositor> CajaGrafica<C> {
    /// Las pantallas públicas, el Workbench primero: (dirección, nombre, privada).
    fn publicas(&self) -> Vec<(u32, Vec<u8>, bool)> {
        let mut v: Vec<(u32, Vec<u8>, bool)> = Vec::new();
        let wb = &self.intui.wb;
        if wb.dir != 0 {
            v.push((wb.dir, wb.nombre.clone().unwrap_or_else(|| b"Workbench".to_vec()), false));
        }
        let mut propias: Vec<&crate::intuition::Pantalla> = self.intui.pantallas.values().filter(|p| p.nombre.is_some()).collect();
        propias.sort_by_key(|p| p.orden);
        v.extend(propias.into_iter().map(|p| (p.dir, p.nombre.clone().unwrap_or_default(), p.privada)));
        v
    }

    /// La pantalla pública de serie: la de `SetDefaultPubScreen` si sigue abierta y no está en modo privado; si no, el
    /// Workbench.
    pub(crate) fn pub_por_defecto(&self) -> u32 {
        let d = self.intui.pub_defecto;
        match self.intui.pantallas.get(&d) {
            Some(p) if p.nombre.is_some() && !p.privada => d,
            _ => self.intui.wb.dir,
        }
    }

    /// Los visitantes de una pantalla: sus bloqueos y las ventanas de otras tareas.
    fn visitantes(&self, s: u32) -> u32 {
        let Some(p) = self.pantalla(s) else { return 0 };
        let ventanas = self.intui.ventanas.values().filter(|v| v.pantalla == s && (p.tarea == 0 || v.tarea != p.tarea)).count();
        p.bloqueos.saturating_add(ventanas as u32)
    }

    /// `LockPubScreenList`.
    pub fn lock_pub_screen_list<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<u32, Fallo> {
        let tarea = ex.tarea_actual();
        if let Some(l) = &mut self.intui.lista_pub {
            l.tareas.push(tarea);
            return Ok(l.lista);
        }
        let todas = self.publicas();
        let mut bloques: Vec<(u32, u32)> = Vec::new();
        let r = self.montar_lista(ex, &todas, &mut bloques);
        match r {
            Ok(lista) => {
                self.intui.lista_pub = Some(ListaPub { lista, bloques, tareas: alloc::vec![tarea] });
                Ok(lista)
            }
            Err(f) => {
                for (d, l) in bloques {
                    ex.soltar(d, l)?;
                }
                Err(f)
            }
        }
    }

    fn montar_lista<M: Memoria>(&self, ex: &mut Exec<M>, todas: &[(u32, Vec<u8>, bool)], bloques: &mut Vec<(u32, u32)>) -> Result<u32, Fallo> {
        let lista = ex.reservar(list::TAMANO, memf::CLEAR)?;
        bloques.push((lista, list::TAMANO));
        // Vacía: la cabeza apunta a la cola, y la de antes de la cola a la cabeza.
        ex.e32(lista + list::LH_HEAD, lista + list::LH_TAIL)?;
        ex.e32(lista + list::LH_TAILPRED, lista)?;
        for (s, nombre, privada) in todas {
            let n = ex.reservar(psn::TAMANO, memf::CLEAR)?;
            bloques.push((n, psn::TAMANO));
            let largo = nombre.len().min(MAXPUBSCREENNAME);
            let texto = ex.reservar(largo as u32 + 1, memf::CLEAR)?;
            bloques.push((texto, largo as u32 + 1));
            ex.escribir_bytes(texto, &nombre[..largo])?;
            let (tarea, bit) = self.pantalla(*s).and_then(|p| p.aviso).unwrap_or((0, 0));
            ex.e32(n + node::LN_NAME, texto)?;
            ex.e32(n + psn::SCREEN, *s)?;
            ex.e16(n + psn::FLAGS, if *privada { PSNF_PRIVATE } else { 0 })?;
            ex.e16(n + psn::SIZE, psn::TAMANO as u16)?;
            ex.e16(n + psn::VISITORCOUNT, self.visitantes(*s).min(i16::MAX as u32) as u16)?;
            ex.e32(n + psn::SIGTASK, tarea)?;
            ex.e8(n + psn::SIGBIT, bit)?;
            // AddTail.
            let ultimo = ex.l32(lista + list::LH_TAILPRED)?;
            ex.e32(n + node::LN_SUCC, lista + list::LH_TAIL)?;
            ex.e32(n + node::LN_PRED, ultimo)?;
            ex.e32(ultimo + node::LN_SUCC, n)?;
            ex.e32(lista + list::LH_TAILPRED, n)?;
        }
        Ok(lista)
    }

    /// `UnlockPubScreenList`: con el último, la foto se suelta.
    pub fn unlock_pub_screen_list<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        let tarea = ex.tarea_actual();
        let Some(l) = &mut self.intui.lista_pub else { return Err(Fallo::DesbloqueoDeMas { dir: 0 }) };
        // La suya si la tiene; si no, una cualquiera (otra tarea puede soltar lo que cogió una hija).
        let i = l.tareas.iter().position(|&t| t == tarea).unwrap_or(0);
        l.tareas.remove(i);
        if l.tareas.is_empty() {
            self.soltar_lista_pub(ex)?;
        }
        Ok(())
    }

    fn soltar_lista_pub<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        if let Some(l) = self.intui.lista_pub.take() {
            for (d, largo) in l.bloques {
                ex.soltar(d, largo)?;
            }
        }
        Ok(())
    }

    /// Lo de la lista que tenía una tarea que acaba: se suelta.
    pub(crate) fn lista_pub_tarea_muerta<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32) {
        let Some(l) = &mut self.intui.lista_pub else { return };
        l.tareas.retain(|&x| x != t);
        if l.tareas.is_empty()
            && let Err(f) = self.soltar_lista_pub(ex)
        {
            self.avisar(ex, "UnlockPubScreenList", f);
        }
    }

    /// Copia un nombre de pantalla (con su 0) en `buf`.
    fn copiar_nombre<M: Memoria>(ex: &mut Exec<M>, buf: u32, nombre: &[u8]) -> Result<(), Fallo> {
        if buf == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let largo = nombre.len().min(MAXPUBSCREENNAME);
        let mut b = nombre[..largo].to_vec();
        b.push(0);
        ex.escribir_bytes(buf, &b)
    }

    /// `NextPubScreen(screen, namebuff)`: el nombre de la pantalla pública que va después de `screen` (dando la vuelta;
    /// las que están en modo privado no cuentan). `namebuff`, o 0 si no hay ninguna.
    pub fn next_pub_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32, buf: u32) -> Result<u32, Fallo> {
        if s != 0 && self.pantalla(s).is_none() {
            return Err(Fallo::PantallaDesconocida { dir: s });
        }
        let abiertas: Vec<(u32, Vec<u8>)> = self.publicas().into_iter().filter(|p| !p.2).map(|p| (p.0, p.1)).collect();
        if abiertas.is_empty() {
            return Ok(0);
        }
        let i = abiertas.iter().position(|p| p.0 == s).map_or(0, |i| (i + 1) % abiertas.len());
        Self::copiar_nombre(ex, buf, &abiertas[i].1)?;
        Ok(buf)
    }

    /// `SetDefaultPubScreen(name)`: la pantalla pública de serie (0: el Workbench). Un nombre que no es de ninguna pantalla
    /// pública abierta no cambia nada.
    pub fn set_default_pub_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, nombre: u32) -> Result<(), Fallo> {
        if nombre == 0 {
            self.intui.pub_defecto = 0;
            return Ok(());
        }
        let n = ex.cadena(nombre)?;
        if let Some(p) = self.publicas().into_iter().find(|p| !p.2 && p.1.eq_ignore_ascii_case(&n)) {
            self.intui.pub_defecto = if p.0 == self.intui.wb.dir { 0 } else { p.0 };
        }
        Ok(())
    }

    /// `GetDefaultPubScreen(namebuff)`: el nombre de la de serie, si `namebuff` no es 0.
    pub fn get_default_pub_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, buf: u32) -> Result<u32, Fallo> {
        if buf == 0 {
            return Ok(0);
        }
        let s = self.pub_por_defecto();
        let nombre = self.pantalla(s).and_then(|p| p.nombre.clone()).unwrap_or_else(|| b"Workbench".to_vec());
        Self::copiar_nombre(ex, buf, &nombre)?;
        Ok(0)
    }

    /// `SetPubScreenModes(modes)`: `SHANGHAI` y `POPPUBSCREEN`; devuelve los de antes.
    pub fn set_pub_screen_modes(&mut self, modos: u32) -> u32 {
        core::mem::replace(&mut self.intui.modos_pub, modos & (SHANGHAI | POPPUBSCREEN))
    }
}
