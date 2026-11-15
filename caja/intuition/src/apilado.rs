//! El orden y el sitio de las pantallas: `ScreenToFront`, `ScreenToBack`, `ScreenDepth`, `MoveScreen`,
//! `ScreenPosition` y lo que llega del compositor para la superficie de una pantalla propia (el ratón, el foco, el
//! arrastre por su barra).
//!
//! Cada pantalla propia es una superficie de primer nivel del compositor y sus ventanas van dentro de ella (hijas):
//! ponerla delante o detrás la lleva con sus ventanas. La de Workbench es el escritorio, con sus ventanas sueltas: ponerla
//! delante es poner detrás las pantallas propias (en su orden), y al revés. La lista de `IntuitionBase` (`FirstScreen`,
//! la de delante primero) sigue el mismo orden.

use alloc::vec::Vec;

use caja_exec::{Exec, Memoria};

use crate::acceso::Acceso;
use crate::compositor::{BTN_LEFT, Evento};
use crate::estructuras::{intuitionbase as ib, screen as sc};
use crate::{CajaGrafica, Compositor, Fallo, Superficie};

/// `ScreenDepth`: al fondo (si no, al frente). `SDEPTH_INFAMILY` (pantallas hijas) no cuenta: no las hay.
const SDEPTH_TOBACK: u32 = 1;
/// `ScreenPosition`: a un sitio (si no, relativo); `SPOS_MAKEVISIBLE`: que se vea ese trozo (ya se ve entera).
const SPOS_ABSOLUTE: u32 = 1;
const SPOS_MAKEVISIBLE: u32 = 2;
/// Tope de pantallas al recorrer la lista de `IntuitionBase` (una lista estropeada no cuelga).
const MAX_PANTALLAS: usize = 4096;

impl<C: Compositor> CajaGrafica<C> {
    /// La lista de pantallas de `IntuitionBase`, de delante atrás. Si el programa la ha estropeado (en círculo, con algo
    /// que no es una pantalla, sin alguna), lo que no es se queda fuera y la que falta va detrás.
    fn lista_pantallas<M: Memoria>(&self, ex: &Exec<M>) -> Result<Vec<u32>, Fallo> {
        let mut v = Vec::new();
        let mut s = ex.l32(self.intui.base + ib::FIRSTSCREEN)?;
        let mut vueltas = 0;
        while s != 0 && vueltas < MAX_PANTALLAS && self.pantalla(s).is_some() && !v.contains(&s) {
            v.push(s);
            s = ex.l32(s + sc::NEXTSCREEN)?;
            vueltas += 1;
        }
        let todas = core::iter::once(self.intui.wb.dir).chain(self.intui.pantallas.keys().copied());
        for t in todas.filter(|&t| t != 0).collect::<Vec<_>>() {
            if !v.contains(&t) {
                v.push(t);
            }
        }
        Ok(v)
    }

    /// Rehace la lista de `IntuitionBase` con este orden.
    fn poner_lista<M: Memoria>(&self, ex: &mut Exec<M>, v: &[u32]) -> Result<(), Fallo> {
        ex.e32(self.intui.base + ib::FIRSTSCREEN, v.first().copied().unwrap_or(0))?;
        for (i, &s) in v.iter().enumerate() {
            ex.e32(s + sc::NEXTSCREEN, v.get(i + 1).copied().unwrap_or(0))?;
        }
        Ok(())
    }

    /// `ScreenToFront` (`frente`) y `ScreenToBack`.
    pub fn screen_depth<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32, frente: bool) -> Result<(), Fallo> {
        if self.pantalla(s).is_none() {
            return Err(Fallo::PantallaDesconocida { dir: s });
        }
        let mut lista = self.lista_pantallas(ex)?;
        lista.retain(|&x| x != s);
        if frente {
            lista.insert(0, s);
        } else {
            lista.push(s);
        }
        self.poner_lista(ex, &lista)?;
        // Al compositor: las superficies de las pantallas propias, sin el Workbench.
        let propias: Vec<Superficie> =
            lista.iter().filter_map(|x| self.intui.pantallas.get(x)).map(|p| p.superficie).filter(|&x| x != 0).collect();
        if s == self.intui.wb.dir {
            // El Workbench delante: las propias detrás de todo, la de delante la última (al fondo de una en una, de delante
            // atrás); detrás: delante de todo, la de delante la última.
            if frente {
                for &x in &propias {
                    self.comp.al_fondo(x);
                }
            } else {
                for &x in propias.iter().rev() {
                    self.comp.al_frente(x);
                }
            }
        } else if let Some(sup) = self.intui.pantallas.get(&s).map(|p| p.superficie).filter(|&x| x != 0) {
            if frente {
                self.comp.al_frente(sup);
            } else {
                self.comp.al_fondo(sup);
            }
        }
        Ok(())
    }

    /// `ScreenDepth(screen, flags, reserved)`.
    pub fn screen_depth_flags<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32, flags: u32) -> Result<(), Fallo> {
        self.screen_depth(ex, s, flags & SDEPTH_TOBACK == 0)
    }

    /// `MoveScreen` y `ScreenPosition`: lleva la pantalla a `(x, y)` (en el escritorio). La de Workbench es el escritorio:
    /// no se mueve.
    fn mover_pantalla<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32, x: i32, y: i32) -> Result<(), Fallo> {
        if s == self.intui.wb.dir {
            return Ok(());
        }
        let Some(sup) = self.intui.pantallas.get(&s).map(|p| p.superficie) else {
            return Err(Fallo::PantallaDesconocida { dir: s });
        };
        let (x, y) = (x.clamp(i16::MIN as i32, i16::MAX as i32), y.clamp(i16::MIN as i32, i16::MAX as i32));
        ex.e16(s + sc::LEFTEDGE, x as u16)?;
        ex.e16(s + sc::TOPEDGE, y as u16)?;
        if sup != 0 {
            self.comp.mover(sup, x, y);
        }
        Ok(())
    }

    fn sitio_pantalla<M: Memoria>(&self, ex: &Exec<M>, s: u32) -> Result<(i32, i32), Fallo> {
        Ok((ex.l16(s + sc::LEFTEDGE)? as i16 as i32, ex.l16(s + sc::TOPEDGE)? as i16 as i32))
    }

    /// `MoveScreen(screen, dx, dy)`.
    pub fn move_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32, dx: i32, dy: i32) -> Result<(), Fallo> {
        if self.pantalla(s).is_none() {
            return Err(Fallo::PantallaDesconocida { dir: s });
        }
        let (x, y) = self.sitio_pantalla(ex, s)?;
        self.mover_pantalla(ex, s, x.saturating_add(dx), y.saturating_add(dy))
    }

    /// `ScreenPosition(screen, flags, x1, y1, x2, y2)`.
    pub fn screen_position<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32, flags: u32, x1: i32, y1: i32) -> Result<(), Fallo> {
        if self.pantalla(s).is_none() {
            return Err(Fallo::PantallaDesconocida { dir: s });
        }
        if flags & SPOS_MAKEVISIBLE != 0 {
            return Ok(());
        }
        if flags & SPOS_ABSOLUTE != 0 {
            return self.mover_pantalla(ex, s, x1, y1);
        }
        self.move_screen(ex, s, x1, y1)
    }

    /// Lo que llega del compositor para la superficie `sup` de una pantalla propia (si lo es): el ratón (`MouseX`,
    /// `MouseY`), pulsar en su barra (arrastrarla) o fuera de sus ventanas (ninguna activa), el foco y el teclado (para su
    /// ventana activa), su sitio nuevo y un búfer perdido.
    pub(crate) fn evento_pantalla<M: Memoria>(&mut self, ex: &mut Exec<M>, sup: Superficie, e: Evento) -> Result<bool, Fallo> {
        let Some((s, activa)) = self.intui.pantallas.values().find(|p| p.superficie == sup).map(|p| (p.dir, p.activa)) else {
            return Ok(false);
        };
        let activa = activa != 0 && self.intui.ventanas.contains_key(&activa);
        let w = self.intui.pantallas[&s].activa;
        match e {
            Evento::Raton { x, y } => {
                ex.e16(s + sc::MOUSEX, x as u16)?;
                ex.e16(s + sc::MOUSEY, y as u16)?;
                let (px, py) = self.sitio_pantalla(ex, s)?;
                ex.e16(self.intui.base + ib::MOUSEX, x.saturating_add(px) as u16)?;
                ex.e16(self.intui.base + ib::MOUSEY, y.saturating_add(py) as u16)?;
                Ok(false)
            }
            Evento::Boton { boton, pulsado } => {
                self.intui.calif.boton(boton, pulsado);
                if boton != BTN_LEFT || !pulsado {
                    return Ok(false);
                }
                let my = ex.l16(s + sc::MOUSEY)? as i16 as i32;
                let barra = ex.l8(s + sc::BARHEIGHT)? as i32;
                if ex.l16(s + sc::FLAGS)? & sc::SHOWTITLE != 0 && (0..=barra).contains(&my) {
                    // Su barra: arrastrarla (con sus ventanas, que van dentro).
                    self.comp.arrastrar(sup);
                }
                if activa { self.activar(ex, w, false) } else { Ok(false) }
            }
            Evento::Foco(si) if activa => self.activar(ex, w, si),
            Evento::Tecla { codigo, pulsada, repeticion, caracter } if activa => {
                self.tecla(ex, w, codigo, pulsada, repeticion, caracter)
            }
            Evento::Tecla { codigo, pulsada, .. } => {
                self.intui.calif.tecla(codigo, pulsada);
                Ok(false)
            }
            Evento::Posicion { x, y } => {
                ex.e16(s + sc::LEFTEDGE, x as u16)?;
                ex.e16(s + sc::TOPEDGE, y as u16)?;
                Ok(false)
            }
            Evento::Exponer => {
                self.exponer_pantalla(ex, s);
                Ok(false)
            }
            _ => Ok(false),
        }
    }
}
