//! `EasyRequestArgs`: el requester sencillo de intuition (autodocs de intuition, "EasyRequestArgs"; RKRM Libraries,
//! "Intuition Requesters and Alerts"). Una ventana con el título, el texto (`es_TextFormat` con sus argumentos, como
//! `RawDoFmt`: `%s`, `%ld`, `%lu`, `%lx`, `%%`; una línea por cada `\n`) y un botón por cada parte de
//! `es_GadgetFormat` separada por `|`. Devuelve 1, 2... por los botones de izquierda a derecha y 0 por el último.
//!
//! La tarea espera (como en `Wait`): el primer `EasyRequestArgs` abre la ventana y la deja esperando la señal
//! `SIGF_SINGLE`; un clic en un botón apunta cuál y le da la señal; la caja repite la llamada, que cierra la ventana y
//! devuelve el botón. Se pinta por el tema (`tema.rs`), como los gadgets de GadTools.

use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Accion, Exec, Memoria, Registros};

use crate::acceso::Acceso;
use crate::estructuras::{tag, wa, wflg};
use crate::gadtools::{Gt, kind};
use crate::lienzo::Rect;
use crate::tema::Vista;
use crate::{CajaGrafica, Compositor, Fallo, fuente};

/// `SIGF_SINGLE`: la señal de las esperas de una sola cosa.
const SIGF_SINGLE: u32 = 1 << 4;
const MAX_LINEAS: usize = 20;
const MAX_LARGO: usize = 80;

pub(crate) struct Requester {
    pub w: u32,
    pub tarea: u32,
    pub lineas: Vec<Vec<u8>>,
    pub botones: Vec<(Rect, Vec<u8>)>,
    pub pulsado: Option<usize>,
    pub resultado: Option<u32>,
}

/// `RawDoFmt` lo justo para `EasyRequestArgs`: `%s`, `%ld`/`%d`, `%lu`/`%u`, `%lx`/`%x`, `%%`, con los argumentos de
/// `args` (LONG o punteros, uno tras otro).
fn formatear<M: Memoria>(ex: &Exec<M>, f: &[u8], args: u32, siguiente: &mut u32) -> Vec<u8> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < f.len() && v.len() < 4096 {
        if f[i] != b'%' || i + 1 >= f.len() {
            v.push(f[i]);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < f.len() && (f[j] == b'-' || f[j].is_ascii_digit() || f[j] == b'.') {
            j += 1;
        }
        let largo = j < f.len() && f[j] == b'l';
        if largo {
            j += 1;
        }
        let Some(&c) = f.get(j) else { break };
        let arg = |n: &mut u32| -> u32 {
            let d = if largo || c == b's' { ex.l32(args.wrapping_add(*n)).unwrap_or(0) } else {
                ex.l16(args.wrapping_add(*n)).unwrap_or(0) as u32
            };
            *n += if largo || c == b's' { 4 } else { 2 };
            d
        };
        match c {
            b'%' => v.push(b'%'),
            b's' => {
                let p = arg(siguiente);
                let mut s = if p != 0 { ex.cadena(p).unwrap_or_default() } else { Vec::new() };
                s.truncate(256);
                v.extend_from_slice(&s);
            }
            b'd' | b'u' | b'x' | b'X' => {
                let n = arg(siguiente);
                let t = match c {
                    b'd' if largo => alloc::format!("{}", n as i32),
                    b'd' => alloc::format!("{}", n as u16 as i16),
                    b'u' => alloc::format!("{}", n),
                    _ => alloc::format!("{:x}", n),
                };
                v.extend_from_slice(t.as_bytes());
            }
            b'c' => {
                let n = arg(siguiente);
                v.push(n as u8);
            }
            _ => v.extend_from_slice(&f[i..=j]),
        }
        i = j + 1;
    }
    v
}

impl<C: Compositor> CajaGrafica<C> {
    /// `EasyRequestArgs(window, easyStruct, idcmpPtr, args)`: ver arriba. La primera vez bloquea a la tarea; la
    /// repetición (cuando la despierta el clic) devuelve el botón.
    pub(crate) fn easy_request<M: Memoria>(&mut self, ex: &mut Exec<M>, r: &mut Registros) -> Accion {
        let t = ex.tarea_actual();
        match self.easy_request_paso(ex, t, r.a[1], r.a[3]) {
            Ok(Some(v)) => {
                r.d[0] = v;
                Accion::Seguir
            }
            Ok(None) => Accion::Bloquear,
            Err(f) => {
                self.avisar(ex, "EasyRequestArgs", f);
                self.cerrar_requester(ex, t);
                r.d[0] = 0;
                Accion::Seguir
            }
        }
    }

    fn easy_request_paso<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32, es: u32, args: u32) -> Result<Option<u32>, Fallo> {
        if let Some(q) = self.requesters.iter().position(|q| q.tarea == t) {
            if let Some(v) = self.requesters[q].resultado {
                self.cerrar_requester(ex, t);
                return Ok(Some(v));
            }
            // Despertada sin clic: sigue esperando.
            ex.set_signal(0, SIGF_SINGLE)?;
            ex.wait(SIGF_SINGLE)?;
            return Ok(None);
        }
        ex.tramo(es, 20)?;
        let titulo = ex.cadena(ex.l32(es.wrapping_add(8))?).unwrap_or_default();
        let fmt_texto = ex.cadena(ex.l32(es.wrapping_add(12))?)?;
        let fmt_botones = ex.cadena(ex.l32(es.wrapping_add(16))?)?;
        let mut n = 0;
        let texto = formatear(ex, &fmt_texto, args, &mut n);
        let botones_t = formatear(ex, &fmt_botones, args, &mut n);
        let mut lineas: Vec<Vec<u8>> = texto.split(|&b| b == b'\n').map(|l| l.iter().take(MAX_LARGO).copied().collect()).collect();
        lineas.truncate(MAX_LINEAS);
        let mut nombres: Vec<Vec<u8>> = botones_t.split(|&b| b == b'|').map(|l| l.iter().take(32).copied().collect()).collect();
        nombres.truncate(8);
        // El tamaño: el texto más ancho o los botones, con márgenes; los bordes de la ventana, los de intuition.
        let (fw, fa) = (fuente::ANCHO as i32, fuente::ALTO as i32);
        let ancho_texto = lineas.iter().map(|l| l.len() as i32 * fw).max().unwrap_or(0);
        let anchos: Vec<i32> = nombres.iter().map(|b| (b.len() as i32 * fw + 24).max(60)).collect();
        let ancho_botones: i32 = anchos.iter().sum::<i32>() + 12 * (anchos.len() as i32 - 1).max(0);
        let dentro_an = ancho_texto.max(ancho_botones) + 40;
        let (bi, ba) = (4, 2 + fa + 1);
        let alto_texto = lineas.len() as i32 * (fa + 2);
        let dentro_al = 12 + alto_texto + 12 + 14 + 8;
        let tit = ex.cadena_nueva(&titulo)?;
        let lista = ex.reservar(8 * 6, memf::CLEAR)?;
        let etiquetas = [
            (wa::TITLE, tit),
            (wa::INNERWIDTH, dentro_an as u32),
            (wa::INNERHEIGHT, dentro_al as u32),
            (wa::FLAGS, wflg::DRAGBAR | wflg::DEPTHGADGET | wflg::ACTIVATE),
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
        // Los botones, centrados abajo.
        let y0 = ba + 12 + alto_texto + 12;
        let mut x = bi + (dentro_an - ancho_botones) / 2;
        let mut botones = Vec::new();
        for (k, nombre) in nombres.into_iter().enumerate() {
            let r = Rect::nuevo(x, y0, x + anchos[k] - 1, y0 + 13);
            x += anchos[k] + 12;
            botones.push((r, nombre));
        }
        self.requesters.push(Requester { w, tarea: t, lineas, botones, pulsado: None, resultado: None });
        self.requester_titulos.push((w, tit, titulo.len() as u32 + 1));
        self.dibujar_requester(ex, w);
        ex.set_signal(0, SIGF_SINGLE)?;
        ex.wait(SIGF_SINGLE)?;
        Ok(None)
    }

    fn cerrar_requester<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32) {
        let Some(q) = self.requesters.iter().position(|q| q.tarea == t) else { return };
        let req = self.requesters.remove(q);
        if self.intui.ventanas.contains_key(&req.w) {
            let _ = self.close_window(ex, req.w);
        }
        if let Some(k) = self.requester_titulos.iter().position(|(w, _, _)| *w == req.w) {
            let (_, d, n) = self.requester_titulos.remove(k);
            let _ = ex.soltar(d, n);
        }
    }

    /// Pinta el requester (por el tema): el texto y los botones.
    pub(crate) fn dibujar_requester<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32) {
        let Some(req) = self.requesters.iter().find(|q| q.w == w) else { return };
        let Some(v) = self.intui.ventanas.get(&w) else { return };
        let bm = v.bm;
        let [bi, ba, _, _] = v.geometria.bordes;
        let fa = fuente::ALTO as i32;
        let mut vistas: Vec<(Gt, Rect)> = Vec::new();
        for (k, l) in req.lineas.iter().enumerate() {
            let y = ba + 12 + k as i32 * (fa + 2);
            let r = Rect::nuevo(bi + 20, y, bi + 20 + l.len() as i32 * fuente::ANCHO as i32, y + fa + 1);
            vistas.push((Gt { tipo: kind::TEXT, texto: l.clone(), ..Default::default() }, r));
        }
        for (k, (r, nombre)) in req.botones.iter().enumerate() {
            let g = Gt {
                tipo: kind::BUTTON,
                etiqueta: nombre.clone(),
                lugar: crate::gadtools::gt::PLACETEXT_IN,
                pulsado: req.pulsado == Some(k),
                ..Default::default()
            };
            vistas.push((g, *r));
        }
        let plumas = self.plumas(ex);
        let tema = self.tema_de(w);
        let glifos = &self.intui.glifos;
        let Some(l) = self.gfx.lienzos.get_mut(&bm) else { return };
        for (g, r) in &vistas {
            let vista = Vista {
                s: g,
                r: *r,
                desactivado: false,
                paleta: &[],
                elementos: &[],
                cadena: &[],
                cursor: 0,
                activo_texto: false,
            };
            let d = tema.como().gadget(l, glifos, &plumas, &vista);
            l.danar(d.interseccion(&l.rect()));
        }
    }

    /// El botón izquierdo en una ventana de requester: `None` si no es de un requester.
    pub(crate) fn requester_boton<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, x: i32, y: i32, pulsado: bool) -> Option<Result<bool, Fallo>> {
        let q = self.requesters.iter().position(|q| q.w == w)?;
        let sobre = self.requesters[q].botones.iter().position(|(r, _)| r.contiene(x, y));
        if pulsado {
            self.requesters[q].pulsado = sobre;
            self.dibujar_requester(ex, w);
            return Some(Ok(false));
        }
        let antes = self.requesters[q].pulsado.take();
        self.dibujar_requester(ex, w);
        let (Some(k), Some(s)) = (antes, sobre) else { return Some(Ok(false)) };
        if k != s {
            return Some(Ok(false));
        }
        let n = self.requesters[q].botones.len();
        let v = if k + 1 == n { 0 } else { k as u32 + 1 };
        self.requesters[q].resultado = Some(v);
        let t = self.requesters[q].tarea;
        Some(ex.signal(t, SIGF_SINGLE).map(|a| a == Accion::Ceder).map_err(Fallo::from))
    }

    /// La tarea murió con un requester abierto (su ventana la cierra intuition con las demás).
    pub(crate) fn requesters_tarea_muerta<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32) {
        self.cerrar_requester(ex, t);
    }
}
