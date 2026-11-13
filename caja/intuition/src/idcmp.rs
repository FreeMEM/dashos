//! Los mensajes IDCMP: de los eventos del compositor a `IntuiMessage`s en el `UserPort` de la ventana (RKRM:
//! Libraries, "Intuition Input and Output Methods").
//!
//! Intuition reparte el mensaje (un `ExtIntuiMessage`, en memoria del sistema), lo manda con `PutMsg` al
//! `UserPort` y apunta que está fuera. El programa lo contesta con `ReplyMsg`, que lo devuelve al `WindowPort`
//! (sin señal: `PA_IGNORE`), e intuition lo recoge y lo suelta la próxima vez que mira (`reclamar`). Como en el
//! Amiga, los `MOUSEMOVE` y las repeticiones de tecla sin contestar tienen tope (`WA_MouseQueue`, `WA_RptQueue`):
//! los que pasan del tope se pierden.

use alloc::vec::Vec;

use caja_exec::estructuras::{memf, message};
use caja_exec::{Accion, Exec, Memoria};

use crate::acceso::Acceso;
use crate::compositor::{BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, Evento};
use crate::estructuras::{idcmp, ie, intuimessage as im, intuitionbase as ib, screen as sc, wflg, window as win};
use crate::marco::Gadget;
use crate::teclado;
use crate::{CajaGrafica, Compositor, Fallo, Superficie};

impl<C: Compositor> CajaGrafica<C> {
    /// Manda un mensaje de clase `clase` a la ventana, si la ventana lo pidió (`IDCMPFlags`) y tiene `UserPort`.
    /// Devuelve si ha despertado a una tarea de más prioridad (la caja tiene que replanificar).
    pub(crate) fn enviar<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        w: u32,
        clase: u32,
        codigo: u16,
        iaddr: u32,
    ) -> Result<bool, Fallo> {
        let v = self.ventana(w)?;
        let (respuesta, cola_raton, cola_rep) = (v.respuesta, v.cola_raton, v.cola_repeticion);
        let calif = self.intui.calif.bits;
        if ex.l32(w + win::IDCMPFLAGS)? & clase == 0 {
            return Ok(false);
        }
        let puerto = ex.l32(w + win::USERPORT)?;
        if puerto == 0 {
            return Ok(false);
        }
        // Topes de la cola.
        if clase == idcmp::MOUSEMOVE {
            let fuera = v.pendientes.values().filter(|&&c| c == idcmp::MOUSEMOVE).count();
            if fuera >= cola_raton as usize {
                return Ok(false);
            }
        }
        if calif & ie::REPEAT != 0 && (clase == idcmp::RAWKEY || clase == idcmp::VANILLAKEY) {
            let fuera = v.pendientes.values().filter(|&&c| c == idcmp::RAWKEY || c == idcmp::VANILLAKEY).count();
            if fuera >= cola_rep as usize {
                return Ok(false);
            }
        }
        let m = ex.reservar(im::TAMANO_EXT, memf::CLEAR)?;
        let (mx, my) = (ex.l16(w + win::MOUSEX)?, ex.l16(w + win::MOUSEY)?);
        let (s, us) = self.intui.hora;
        ex.e32(m + message::MN_REPLYPORT, respuesta)?;
        ex.e16(m + message::MN_LENGTH, im::TAMANO_EXT as u16)?;
        ex.e32(m + im::CLASS, clase)?;
        ex.e16(m + im::CODE, codigo)?;
        ex.e16(m + im::QUALIFIER, calif)?;
        ex.e32(m + im::IADDRESS, iaddr)?;
        ex.e16(m + im::MOUSEX, mx)?;
        ex.e16(m + im::MOUSEY, my)?;
        ex.e32(m + im::SECONDS, s)?;
        ex.e32(m + im::MICROS, us)?;
        ex.e32(m + im::IDCMPWINDOW, w)?;
        match ex.put_msg(puerto, m) {
            Ok(a) => {
                if let Some(v) = self.intui.ventanas.get_mut(&w) {
                    v.pendientes.insert(m, clase);
                }
                Ok(a == Accion::Ceder)
            }
            Err(f) => {
                ex.soltar(m, im::TAMANO_EXT)?;
                Err(f.into())
            }
        }
    }

    /// Lo que la caja llama cuando el compositor tiene algo: recoge los mensajes contestados, reparte los eventos
    /// en mensajes IDCMP y manda al compositor lo que ha cambiado. Devuelve si alguna tarea de más prioridad se ha
    /// despertado (la caja tiene que pasar el testigo con `Exec::elegir_siguiente`).
    pub fn atender<M: Memoria>(&mut self, ex: &mut Exec<M>) -> bool {
        self.reclamar_todo(ex);
        let mut replan = false;
        for (s, e) in self.comp.sacar_eventos() {
            match self.evento(ex, s, e) {
                Ok(r) => replan |= r,
                Err(f) => self.avisar(ex, "(evento)", f),
            }
        }
        // S26: si ha cambiado el tema de DashOS o el ajuste del aspecto, se repintan las ventanas.
        let v = self.comp.version_tema();
        if v != self.version_tema {
            self.version_tema = v;
            self.repintar_tema(ex);
        }
        self.presentar();
        replan
    }

    /// Repinta lo que pinta intuition en cada ventana con el tema que le toca ahora: los bordes, los gadgets de
    /// GadTools y los requesters. Lo de dentro que era del color de fondo de antes pasa al de ahora (lo que pintó el
    /// programa con otros colores se queda; un menú abierto se ve como estaba hasta que se mueva).
    pub(crate) fn repintar_tema<M: Memoria>(&mut self, ex: &mut Exec<M>) {
        for w in self.ventanas() {
            let Some(v) = self.intui.ventanas.get(&w) else { continue };
            let (bm, viejo, [bi, ba, bd, bb]) = (v.bm, v.fondo, v.geometria.bordes);
            // Si para esta ventana no ha cambiado nada (otro evento del tema con lo mismo), no se toca.
            let aspecto = self.comp.tema_amiga(&v.programa);
            if aspecto == v.aspecto {
                continue;
            }
            if let Some(v) = self.intui.ventanas.get_mut(&w) {
                v.aspecto = aspecto;
            }
            let nuevo = match self.tema_de(w).como().fondo_ventana() {
                Some(f) => f | 0xFF00_0000,
                None => self.color(self.intui.wb.cm, 0) | 0xFF00_0000,
            };
            if nuevo != viejo {
                if let Some(l) = self.gfx.lienzos.get_mut(&bm) {
                    let (an, al) = (l.ancho() as i32, l.alto() as i32);
                    for y in ba.max(0)..(al - bb).min(al) {
                        for x in bi.max(0)..(an - bd).min(an) {
                            if l.pixel(x, y) | 0xFF00_0000 == viejo {
                                l.poner(x, y, nuevo);
                            }
                        }
                    }
                    l.poner_fondo(nuevo);
                }
                if let Some(v) = self.intui.ventanas.get_mut(&w) {
                    v.fondo = nuevo;
                }
            }
            self.dibujar_marco(w);
            let primero = ex.l32(w.wrapping_add(win::FIRSTGADGET)).unwrap_or(0);
            self.dibujar_lista(ex, w, primero, -1, true);
            if self.requesters.iter().any(|q| q.w == w) {
                self.dibujar_requester(ex, w);
            }
        }
    }

    /// Un evento del compositor para la ventana de la superficie `s`.
    pub fn evento<M: Memoria>(&mut self, ex: &mut Exec<M>, s: Superficie, e: Evento) -> Result<bool, Fallo> {
        let Some(&w) = self.intui.por_superficie.get(&s) else { return Ok(false) };
        match e {
            Evento::Raton { x, y } => self.raton(ex, w, x, y),
            Evento::Boton { boton, pulsado } => self.boton(ex, w, boton, pulsado),
            Evento::Tecla { codigo, pulsada, repeticion, caracter } => {
                self.tecla(ex, w, codigo, pulsada, repeticion, caracter)
            }
            Evento::Foco(si) => self.activar(ex, w, si),
            Evento::Tamano { ancho, alto } => self.nuevo_tamano(ex, w, ancho, alto),
            Evento::Posicion { x, y } => {
                ex.e16(w + win::LEFTEDGE, x as u16)?;
                ex.e16(w + win::TOPEDGE, y as u16)?;
                self.enviar(ex, w, idcmp::CHANGEWINDOW, 0, 0)
            }
            Evento::Cerrar => self.enviar(ex, w, idcmp::CLOSEWINDOW, 0, 0),
            Evento::Exponer => {
                let bm = self.ventana(w)?.bm;
                if let Some(l) = self.gfx.lienzos.get_mut(&bm) {
                    let r = l.rect();
                    l.danar(r);
                }
                Ok(false)
            }
        }
    }

    fn raton<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, x: i32, y: i32) -> Result<bool, Fallo> {
        let v = self.ventana(w)?;
        let [bi, ba, _, _] = v.geometria.bordes;
        let arrastrando = v.pulsado.is_some();
        ex.e16(w + win::MOUSEX, x as u16)?;
        ex.e16(w + win::MOUSEY, y as u16)?;
        ex.e16(w + win::GZZMOUSEX, x.saturating_sub(bi) as u16)?;
        ex.e16(w + win::GZZMOUSEY, y.saturating_sub(ba) as u16)?;
        let (wx, wy) = (ex.l16(w + win::LEFTEDGE)? as i16 as i32, ex.l16(w + win::TOPEDGE)? as i16 as i32);
        let s = self.intui.wb.dir;
        ex.e16(s + sc::MOUSEX, x.saturating_add(wx) as u16)?;
        ex.e16(s + sc::MOUSEY, y.saturating_add(wy) as u16)?;
        ex.e16(self.intui.base + ib::MOUSEX, x.saturating_add(wx) as u16)?;
        ex.e16(self.intui.base + ib::MOUSEY, y.saturating_add(wy) as u16)?;
        if self.mover_menu(ex, w, x, y) {
            return Ok(false);
        }
        if let Some(r) = self.gadget_mover(ex, w, x, y) {
            return r;
        }
        if arrastrando || ex.l32(w + win::FLAGS)? & wflg::REPORTMOUSE == 0 {
            return Ok(false);
        }
        self.enviar(ex, w, idcmp::MOUSEMOVE, ie::IECODE_NOBUTTON, 0)
    }

    fn boton<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, boton: u32, pulsado: bool) -> Result<bool, Fallo> {
        self.intui.calif.boton(boton, pulsado);
        let v = self.ventana(w)?;
        let (s, g) = (v.superficie, v.geometria);
        let (x, y) = (ex.l16(w + win::MOUSEX)? as i16 as i32, ex.l16(w + win::MOUSEY)? as i16 as i32);
        if boton == BTN_LEFT
            && let Some(r) = self.requester_boton(ex, w, x, y, pulsado)
        {
            return r;
        }
        match boton {
            BTN_LEFT if pulsado => match g.zona(x, y) {
                Some(Gadget::Arrastre) => {
                    self.comp.arrastrar(s);
                    self.marcar(w, Some(Gadget::Arrastre));
                    Ok(false)
                }
                Some(Gadget::Tamano) => {
                    self.comp.redimensionar(s);
                    self.marcar(w, Some(Gadget::Tamano));
                    Ok(false)
                }
                Some(z) => {
                    self.marcar(w, Some(z));
                    Ok(false)
                }
                None => match self.gadget_pulsar(ex, w, x, y) {
                    Some(r) => r,
                    None => self.enviar(ex, w, idcmp::MOUSEBUTTONS, ie::SELECTDOWN, 0),
                },
            },
            BTN_LEFT => {
                let antes = self.intui.ventanas.get_mut(&w).and_then(|v| v.pulsado.take());
                match antes {
                    Some(Gadget::Cerrar) if g.zona(x, y) == Some(Gadget::Cerrar) => {
                        self.enviar(ex, w, idcmp::CLOSEWINDOW, 0, 0)
                    }
                    Some(Gadget::Profundidad) if g.zona(x, y) == Some(Gadget::Profundidad) => {
                        self.comp.cambiar_profundidad(s);
                        Ok(false)
                    }
                    Some(_) => Ok(false),
                    None => match self.gadget_soltar(ex, w, x, y) {
                        Some(r) => r,
                        None => self.enviar(ex, w, idcmp::MOUSEBUTTONS, ie::SELECTUP, 0),
                    },
                }
            }
            // El botón derecho: los menús de la ventana (menus.rs), o el botón tal cual con `RMBTRAP`.
            BTN_RIGHT => {
                if ex.l32(w + win::FLAGS)? & wflg::RMBTRAP == 0 {
                    if pulsado {
                        self.abrir_menu(ex, w);
                        return Ok(false);
                    }
                    return self.soltar_menu(ex).unwrap_or(Ok(false));
                }
                self.enviar(ex, w, idcmp::MOUSEBUTTONS, if pulsado { ie::MENUDOWN } else { ie::MENUUP }, 0)
            }
            BTN_MIDDLE => {
                self.enviar(ex, w, idcmp::MOUSEBUTTONS, if pulsado { ie::MIDDLEDOWN } else { ie::MIDDLEUP }, 0)
            }
            _ => Ok(false),
        }
    }

    fn marcar(&mut self, w: u32, g: Option<Gadget>) {
        if let Some(v) = self.intui.ventanas.get_mut(&w) {
            v.pulsado = g;
        }
    }

    fn tecla<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        w: u32,
        codigo: u32,
        pulsada: bool,
        repeticion: bool,
        caracter: Option<char>,
    ) -> Result<bool, Fallo> {
        self.intui.calif.tecla(codigo, pulsada);
        let crudo = teclado::crudo(codigo);
        let base = self.intui.calif.bits & !(ie::REPEAT | ie::NUMERICPAD);
        let mut q = base;
        if repeticion {
            q |= ie::REPEAT;
        }
        if crudo.is_some_and(|(_, num)| num) {
            q |= ie::NUMERICPAD;
        }
        self.intui.calif.bits = q;
        if pulsada {
            let ch = match caracter {
                Some(c) if (c as u32) <= 0xFF => Some(c as u32 as u8),
                Some(_) => None,
                None => teclado::caracter_por_defecto(codigo, self.intui.calif),
            };
            if let Some(r) = self.gadget_tecla(ex, w, crudo.map(|(c, _)| c), ch) {
                self.intui.calif.bits = base;
                return r;
            }
        }
        let flags = ex.l32(w + win::IDCMPFLAGS)?;
        let r = (|| {
            if pulsada && flags & idcmp::VANILLAKEY != 0 {
                let c = match caracter {
                    Some(c) if (c as u32) <= 0xFF => Some(c as u32 as u8),
                    Some(_) => None,
                    None => teclado::caracter_por_defecto(codigo, self.intui.calif),
                };
                if let Some(c) = c {
                    return self.enviar(ex, w, idcmp::VANILLAKEY, c as u16, 0);
                }
            }
            if flags & idcmp::RAWKEY != 0
                && let Some((c, _)) = crudo
            {
                let codigo = c as u16 | if pulsada { 0 } else { ie::IECODE_UP_PREFIX };
                let ceros = self.intui.ceros;
                return self.enviar(ex, w, idcmp::RAWKEY, codigo, ceros);
            }
            Ok(false)
        })();
        self.intui.calif.bits = base;
        r
    }

    /// Los mensajes que una ventana tiene fuera (sin recoger), por clase.
    pub fn mensajes_fuera(&self, w: u32) -> Vec<u32> {
        self.intui.ventanas.get(&w).map(|v| v.pendientes.values().copied().collect()).unwrap_or_default()
    }
}
