//! El pedidor de ficheros de la caja (la ventana de `AslRequest` de asl.library nativa, que lleva el pegamento: él
//! lee los cajones con la dos de la caja y decide; aquí solo se pinta y se atiende al ratón y al teclado).
//!
//! Una ventana con la lista del cajón (los cajones primero, con "/" detrás), el cajón y el fichero en dos campos de
//! texto, y los botones "Aceptar" (o lo que pida el programa), "Volúmenes", "Padre" y "Cancelar", pintada por el tema
//! como los gadgets de GadTools. Un clic en una fila la elige (un fichero pasa al campo del fichero); dos seguidos en
//! un cajón entran en él y en un fichero aceptan; Intro en un campo acepta (en el del cajón, entra en lo escrito).
//! Lo que pasa queda en `accion` y la tarea que espera recibe `SIGF_SINGLE`, como con `EasyRequestArgs`.
//!
//! Escrito del autodoc de asl.library (lo que hace el pedidor, no cómo se ve: se ve como DashOS).

use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Memoria};

use crate::acceso::Acceso;
use crate::estructuras::{tag, wa, wflg};
use crate::gadtools::{Gt, gt, kind};
use crate::lienzo::{Pinta, Rect};
use crate::tema::Vista;
use crate::{CajaGrafica, Compositor, Fallo, fuente};

const SIGF_SINGLE: u32 = 1 << 4;
const MAX_CAMPO: usize = 255;

/// Lo que hizo la persona en el pedidor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccionAsl {
    /// Entrar en el cajón de esa fila (doble clic en un cajón) o en el que dice el campo del cajón (Intro en él).
    Entrar(Option<usize>),
    Aceptar,
    Cancelar,
    Padre,
    Volumenes,
}

pub(crate) struct Pedidor {
    pub w: u32,
    pub tarea: u32,
    titulo: (u32, u32),
    entradas: Vec<(Vec<u8>, bool)>,
    textos: Vec<Vec<u8>>,
    elegida: i32,
    arriba: i32,
    cajon: Vec<u8>,
    fichero: Vec<u8>,
    /// El campo con el cursor: 1 el cajón, 2 el fichero.
    campo: u8,
    botones: Vec<(Rect, Vec<u8>, AccionAsl)>,
    lista: Rect,
    r_cajon: Rect,
    r_fichero: Rect,
    pulsado: Option<usize>,
    ultimo_clic: Option<(i32, u64)>,
    accion: Option<AccionAsl>,
}

impl<C: Compositor> CajaGrafica<C> {
    /// Abre el pedidor de la tarea `t` con su título, el texto del botón de aceptar, el cajón, el fichero y lo que hay
    /// en el cajón (nombre y si es un cajón). La tarea espera después `SIGF_SINGLE` (la caja repite su llamada).
    #[allow(clippy::too_many_arguments)]
    pub fn pedidor_abrir<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        t: u32,
        titulo: &[u8],
        aceptar: &[u8],
        cajon: &[u8],
        fichero: &[u8],
        entradas: Vec<(Vec<u8>, bool)>,
    ) -> Result<(), Fallo> {
        self.pedidor_cerrar(ex, t);
        let (fw, fa) = (fuente::ANCHO as i32, fuente::ALTO as i32);
        let (an, al) = (52 * fw, 30 * (fa + 1));
        let tit = ex.cadena_nueva(titulo)?;
        let lista = ex.reservar(8 * 6, memf::CLEAR)?;
        let etiquetas = [
            (wa::TITLE, tit),
            (wa::INNERWIDTH, an as u32),
            (wa::INNERHEIGHT, al as u32),
            (wa::FLAGS, wflg::DRAGBAR | wflg::DEPTHGADGET | wflg::ACTIVATE | wflg::CLOSEGADGET),
            (wa::IDCMP, 0),
            (tag::TAG_DONE, 0),
        ];
        for (k, (a, b)) in etiquetas.iter().enumerate() {
            ex.e32(lista + 8 * k as u32, *a)?;
            ex.e32(lista + 8 * k as u32 + 4, *b)?;
        }
        let w = self.open_window_tag_list(ex, 0, lista);
        ex.soltar(lista, 8 * 6)?;
        let w = match w {
            Ok(w) => w,
            Err(f) => {
                ex.soltar(tit, titulo.len() as u32 + 1)?;
                return Err(f);
            }
        };
        let [bi, ba, _, _] = self.ventana(w)?.geometria.bordes;
        let (x0, y0) = (bi + 8, ba + 8);
        let ancho = an - 16;
        let alto_boton = 14;
        let lista = Rect::nuevo(x0, y0, x0 + ancho - 1, y0 + al - 16 - 3 * (alto_boton + 6) - 1);
        let y_cajon = lista.y1 + 6;
        let r_cajon = Rect::nuevo(x0 + 9 * fw, y_cajon, x0 + ancho - 1, y_cajon + alto_boton - 1);
        let y_fichero = y_cajon + alto_boton + 6;
        let r_fichero = Rect::nuevo(x0 + 9 * fw, y_fichero, x0 + ancho - 1, y_fichero + alto_boton - 1);
        let y_botones = y_fichero + alto_boton + 6;
        let nombres: [(&[u8], AccionAsl); 4] = [
            (aceptar, AccionAsl::Aceptar),
            (b"Volumenes", AccionAsl::Volumenes),
            (b"Padre", AccionAsl::Padre),
            (b"Cancelar", AccionAsl::Cancelar),
        ];
        let paso = (ancho + 8) / 4;
        let botones = nombres
            .iter()
            .enumerate()
            .map(|(k, (n, a))| {
                let x = x0 + k as i32 * paso;
                (Rect::nuevo(x, y_botones, x + paso - 9, y_botones + alto_boton - 1), n.iter().take(24).copied().collect(), *a)
            })
            .collect();
        let mut p = Pedidor {
            w,
            tarea: t,
            titulo: (tit, titulo.len() as u32 + 1),
            entradas: Vec::new(),
            textos: Vec::new(),
            elegida: -1,
            arriba: 0,
            cajon: Vec::new(),
            fichero: fichero.iter().take(MAX_CAMPO).copied().collect(),
            campo: 2,
            botones,
            lista,
            r_cajon,
            r_fichero,
            pulsado: None,
            ultimo_clic: None,
            accion: None,
        };
        p.poner(cajon, entradas);
        self.pedidores.push(p);
        self.dibujar_pedidor(ex, w);
        Ok(())
    }

    /// Lo que hizo la persona (una vez), con el cajón y el fichero de los campos. `None` si aún nada.
    pub fn pedidor_accion(&mut self, t: u32) -> Option<(AccionAsl, Vec<u8>, Vec<u8>)> {
        let p = self.pedidores.iter_mut().find(|p| p.tarea == t)?;
        let a = p.accion.take()?;
        let sel = match a {
            AccionAsl::Entrar(Some(i)) => p.entradas.get(i).map(|e| e.0.clone()).unwrap_or_default(),
            _ => p.fichero.clone(),
        };
        Some((a, p.cajon.clone(), sel))
    }

    /// Si la tarea `t` tiene un pedidor abierto.
    pub fn pedidor_abierto(&self, t: u32) -> bool {
        self.pedidores.iter().any(|p| p.tarea == t)
    }

    /// Cambia el cajón que se ve (tras entrar en otro, el padre o los volúmenes).
    pub fn pedidor_poner<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32, cajon: &[u8], entradas: Vec<(Vec<u8>, bool)>) {
        let Some(p) = self.pedidores.iter_mut().find(|p| p.tarea == t) else { return };
        p.poner(cajon, entradas);
        let w = p.w;
        self.dibujar_pedidor(ex, w);
    }

    pub fn pedidor_cerrar<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32) {
        let Some(k) = self.pedidores.iter().position(|p| p.tarea == t) else { return };
        let p = self.pedidores.remove(k);
        if self.intui.ventanas.contains_key(&p.w) {
            let _ = self.close_window(ex, p.w);
        }
        let _ = ex.soltar(p.titulo.0, p.titulo.1);
    }

    pub(crate) fn es_pedidor(&self, w: u32) -> bool {
        self.pedidores.iter().any(|p| p.w == w)
    }

    /// Pinta el pedidor por el tema.
    pub(crate) fn dibujar_pedidor<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32) {
        let Some(p) = self.pedidores.iter().find(|p| p.w == w) else { return };
        let Some(v) = self.intui.ventanas.get(&w) else { return };
        let bm = v.bm;
        let fw = fuente::ANCHO as i32;
        let lista = Gt { tipo: kind::LISTVIEW, valor: p.elegida, arriba: p.arriba, ..Default::default() };
        let campo = |e: &[u8]| Gt { tipo: kind::STRING, etiqueta: e.to_vec(), lugar: gt::PLACETEXT_LEFT, ..Default::default() };
        let (gc, gf) = (campo(b"Cajon"), campo(b"Fichero"));
        let botones: Vec<Gt> = p
            .botones
            .iter()
            .enumerate()
            .map(|(k, (_, n, _))| Gt {
                tipo: kind::BUTTON,
                etiqueta: n.clone(),
                lugar: gt::PLACETEXT_IN,
                pulsado: p.pulsado == Some(k),
                ..Default::default()
            })
            .collect();
        let plumas = self.plumas(ex, w);
        let tema = self.tema_de(w);
        let glifos = &self.intui.glifos;
        let Some(mut l) = crate::graphics::vista(&mut self.gfx.lienzos, ex, bm) else { return };
        let l = &mut l;
        let mut vistas: Vec<(&Gt, Rect, &[Vec<u8>], &[u8], bool)> = Vec::new();
        vistas.push((&lista, p.lista, &p.textos, &[], false));
        // Las etiquetas de los campos van a su izquierda: el rectángulo de cada campo empieza 9 letras más allá.
        let _ = fw;
        vistas.push((&gc, p.r_cajon, &[], &p.cajon, p.campo == 1));
        vistas.push((&gf, p.r_fichero, &[], &p.fichero, p.campo == 2));
        for (k, g) in botones.iter().enumerate() {
            vistas.push((g, p.botones[k].0, &[], &[], false));
        }
        for (g, r, elementos, cadena, activo) in vistas {
            let vista = Vista {
                s: g,
                r,
                desactivado: false,
                paleta: &[],
                elementos,
                cadena,
                cursor: cadena.len(),
                activo_texto: activo,
            };
            let d = tema.como().gadget(l, glifos, &plumas, &vista);
            l.danar(d.interseccion(&l.rect()));
        }
    }

    /// El botón izquierdo en la ventana de un pedidor. `None` si no es de un pedidor.
    pub(crate) fn pedidor_boton<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, x: i32, y: i32, pulsado: bool, ahora_us: u64) -> Option<Result<bool, Fallo>> {
        let k = self.pedidores.iter().position(|p| p.w == w)?;
        let p = &mut self.pedidores[k];
        let sobre = p.botones.iter().position(|(r, _, _)| r.contiene(x, y));
        if pulsado {
            p.pulsado = sobre;
            if p.lista.contiene(x, y) {
                p.clic_en_lista(x, y, ahora_us);
            } else if p.r_cajon.contiene(x, y) {
                p.campo = 1;
            } else if p.r_fichero.contiene(x, y) {
                p.campo = 2;
            }
        } else {
            let antes = p.pulsado.take();
            if let (Some(a), Some(s)) = (antes, sobre)
                && a == s
            {
                p.accion = Some(p.botones[s].2);
            }
        }
        let (hay, t) = (p.accion.is_some(), p.tarea);
        self.dibujar_pedidor(ex, w);
        if !hay {
            return Some(Ok(false));
        }
        Some(ex.signal(t, SIGF_SINGLE).map(|a| a == caja_exec::Accion::Ceder).map_err(Fallo::from))
    }

    /// Una tecla en la ventana de un pedidor (el campo con el cursor): letras, borrar, Intro, Esc, Tab y las flechas
    /// arriba y abajo en la lista. `None` si no es de un pedidor.
    pub(crate) fn pedidor_tecla<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, crudo: Option<u8>, ch: Option<u8>) -> Option<Result<bool, Fallo>> {
        let k = self.pedidores.iter().position(|p| p.w == w)?;
        let p = &mut self.pedidores[k];
        match (crudo, ch) {
            (_, Some(b'\r')) | (_, Some(b'\n')) => {
                p.accion = Some(if p.campo == 1 { AccionAsl::Entrar(None) } else { AccionAsl::Aceptar });
            }
            (_, Some(0x1B)) => p.accion = Some(AccionAsl::Cancelar),
            (_, Some(b'\t')) => p.campo = if p.campo == 1 { 2 } else { 1 },
            (_, Some(8)) => {
                let c = if p.campo == 1 { &mut p.cajon } else { &mut p.fichero };
                c.pop();
            }
            // Flechas arriba y abajo (códigos crudos del Amiga 0x4C y 0x4D): mueven la fila elegida.
            (Some(0x4C), _) | (Some(0x4D), _) => {
                let n = p.entradas.len() as i32;
                if n > 0 {
                    let d = if crudo == Some(0x4C) { -1 } else { 1 };
                    p.elegida = (p.elegida + d).clamp(0, n - 1);
                    p.a_la_vista();
                    p.elegir(p.elegida as usize);
                }
            }
            (_, Some(c)) if c >= 0x20 && c != 0x7F => {
                let campo = if p.campo == 1 { &mut p.cajon } else { &mut p.fichero };
                if campo.len() < MAX_CAMPO {
                    campo.push(c);
                }
            }
            _ => return Some(Ok(false)),
        }
        let (hay, t) = (p.accion.is_some(), p.tarea);
        self.dibujar_pedidor(ex, w);
        if !hay {
            return Some(Ok(false));
        }
        Some(ex.signal(t, SIGF_SINGLE).map(|a| a == caja_exec::Accion::Ceder).map_err(Fallo::from))
    }

    /// Cerrar desde la barra: como "Cancelar".
    pub(crate) fn pedidor_cerrado<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32) -> Option<Result<bool, Fallo>> {
        let p = self.pedidores.iter_mut().find(|p| p.w == w)?;
        p.accion = Some(AccionAsl::Cancelar);
        let t = p.tarea;
        Some(ex.signal(t, SIGF_SINGLE).map(|a| a == caja_exec::Accion::Ceder).map_err(Fallo::from))
    }

    pub(crate) fn pedidores_tarea_muerta<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32) {
        self.pedidor_cerrar(ex, t);
    }
}

impl Pedidor {
    fn poner(&mut self, cajon: &[u8], mut entradas: Vec<(Vec<u8>, bool)>) {
        // Los cajones primero; dentro de cada grupo, por nombre sin mirar mayúsculas.
        entradas.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase())));
        self.textos = entradas
            .iter()
            .map(|(n, c)| {
                let mut t: Vec<u8> = n.clone();
                if *c {
                    t.push(b'/');
                }
                t
            })
            .collect();
        self.entradas = entradas;
        self.cajon = cajon.iter().take(MAX_CAMPO).copied().collect();
        self.elegida = -1;
        self.arriba = 0;
        self.ultimo_clic = None;
    }

    fn filas(&self) -> i32 {
        ((self.lista.y1 - self.lista.y0 - 3) / (fuente::ALTO as i32 + 1)).max(1)
    }

    fn a_la_vista(&mut self) {
        let f = self.filas();
        if self.elegida < self.arriba {
            self.arriba = self.elegida;
        } else if self.elegida >= self.arriba + f {
            self.arriba = self.elegida - f + 1;
        }
    }

    fn elegir(&mut self, i: usize) {
        if let Some((n, false)) = self.entradas.get(i) {
            self.fichero = n.clone();
        }
    }

    fn clic_en_lista(&mut self, x: i32, y: i32, ahora_us: u64) {
        let n = self.entradas.len() as i32;
        let f = self.filas();
        // La barra de la derecha: arriba o abajo una página.
        if x > self.lista.x1 - 16 {
            let medio = (self.lista.y0 + self.lista.y1) / 2;
            self.arriba = if y < medio { self.arriba - f } else { self.arriba + f }.clamp(0, (n - f).max(0));
            return;
        }
        let fila = (y - self.lista.y0 - 2) / (fuente::ALTO as i32 + 1) + self.arriba;
        if fila < 0 || fila >= n {
            return;
        }
        let doble = self.ultimo_clic.is_some_and(|(i, t)| i == fila && ahora_us.saturating_sub(t) <= crate::DOBLE_CLIC_US as u64);
        self.elegida = fila;
        self.elegir(fila as usize);
        self.ultimo_clic = Some((fila, ahora_us));
        if doble {
            self.ultimo_clic = None;
            self.accion = Some(if self.entradas[fila as usize].1 { AccionAsl::Entrar(Some(fila as usize)) } else { AccionAsl::Aceptar });
        }
    }
}
