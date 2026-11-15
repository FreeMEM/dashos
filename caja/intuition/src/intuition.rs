//! intuition.library: la pantalla de Workbench y las ventanas (RKRM: Libraries, "Intuition Screens", "Intuition
//! Windows", "Intuition Input and Output Methods"; autodocs de intuition.library).
//!
//! Cada ventana tiene sus estructuras en la memoria del 68k (`Window`, su `RastPort`, una `Layer` y un `BitMap`)
//! y, fuera, un lienzo nativo del tamaño de la ventana entera y una superficie del compositor. A diferencia del
//! Amiga, `Window->RPort->BitMap` no es el de la pantalla: es el de la ventana, y las coordenadas del `RastPort`
//! son las de la ventana (como en el Amiga, que las traslada con la capa). Lo que el programa pinta va al búfer
//! de su ventana y de ahí al compositor.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_exec::estructuras::{memf, msgport, node, nt};
use caja_exec::{Exec, Memoria};

use crate::acceso::{Acceso, palabra, puntero};
use crate::compositor::NuevaVentana;
use crate::estructuras::{
    cliprect, colormap, drawinfo as dri, idcmp, intuitionbase as ib, layer, newwindow as nw, rastport as rp_, rectangle,
    screen as sc, tag, textattr, textfont, tipo_pantalla, viewport, wa, wflg, window as win,
};
use crate::lienzo::{self, Pinta, Rect};
use crate::marco::{GADGET_TAMANO, Gadget, Geometria};
use crate::teclado::Calificadores;
use crate::{CajaGrafica, Compositor, Fallo, Superficie, Tabla, fuente, lvo};

/// Una pantalla: la de Workbench o una propia de un programa (`pantallas.rs`).
#[derive(Default)]
pub(crate) struct Pantalla {
    pub dir: u32,
    pub ancho: u32,
    pub alto: u32,
    pub cm: u32,
    pub drawinfo: u32,
    pub bloqueos: u32,
    pub plumas: [u16; dri::NUMDRIPENS],
    /// Su `ModeID` (uno de la caja: `modos.rs`).
    pub modo: u32,
    /// La tarea que la abrió (0: la de Workbench) y lo reservado para ella (dirección, largo), para soltarlo.
    pub tarea: u32,
    pub bloques: Vec<(u32, u32)>,
}

/// Lo que intuition sabe de una ventana fuera de la memoria del 68k.
pub(crate) struct Ventana {
    pub rp: u32,
    pub capa: u32,
    /// La `ClipRect` de la capa (la capa entera) y su `DamageList` (lo que falta por pintar), del sistema.
    pub cliprect: u32,
    pub dano: u32,
    pub bm: u32,
    /// Su pantalla (la de Workbench o una propia).
    pub pantalla: u32,
    pub superficie: Superficie,
    /// El `UserPort` que creó intuition (0 si es del programa o no hay).
    pub puerto_creado: u32,
    /// El `WindowPort`: adonde vuelven los mensajes contestados.
    pub respuesta: u32,
    /// Mensajes enviados y aún no recuperados: mensaje → clase.
    pub pendientes: BTreeMap<u32, u32>,
    pub tarea: u32,
    pub orden: u64,
    pub titulo: Vec<u8>,
    /// El programa que la abrió (el nombre de su tarea): de él sale su aspecto (S26).
    pub programa: alloc::string::String,
    /// El color de dentro con el que se abrió (el de la pluma 0, o la superficie de DashOS).
    pub fondo: u32,
    /// Los colores de DashOS con los que se pintó (`None`: el clásico): si no cambian, no se repinta.
    pub aspecto: Option<crate::compositor::Tokens>,
    pub activa: bool,
    pub geometria: Geometria,
    pub pulsado: Option<Gadget>,
    pub cola_raton: u16,
    pub cola_repeticion: u16,
    pub minimo: (i32, i32),
    pub maximo: (i32, i32),
    /// La ventana ya se cerró y espera a que vuelvan sus mensajes.
    pub dir: u32,
}

#[derive(Default)]
pub(crate) struct Estado {
    pub base: u32,
    pub wb: Pantalla,
    /// Las pantallas propias de los programas (`OpenScreenTagList` con un modo de la caja).
    pub pantallas: Tabla<Pantalla>,
    pub ventanas: Tabla<Ventana>,
    pub por_superficie: BTreeMap<Superficie, u32>,
    pub cerradas: Vec<Ventana>,
    pub hora: (u32, u32),
    pub calif: Calificadores,
    /// Ocho bytes a cero: el `IAddress` de los `RAWKEY` (las teclas anteriores, para `MapRawKey`).
    pub ceros: u32,
    pub orden: u64,
    pub glifos: Vec<[u8; 8]>,
}

/// Lo que pide `OpenWindow` u `OpenWindowTagList`.
struct Peticion {
    izq: i32,
    arr: i32,
    ancho: i32,
    alto: i32,
    detalle: u8,
    bloque: u8,
    idcmp: u32,
    flags: u32,
    titulo: u32,
    titulo_pantalla: u32,
    pantalla: Option<u32>,
    nombre_pub: Option<u32>,
    respaldo: bool,
    minimo: (i32, i32),
    maximo: (i32, i32),
    interior: (Option<i32>, Option<i32>),
    cola_raton: u16,
    cola_repeticion: u16,
    /// Los gadgets del programa (`WA_Gadgets`, `nw_FirstGadget`).
    gadgets: u32,
}

/// Tope de etiquetas que se recorren (más es una lista en círculo).
const MAX_ETIQUETAS: u32 = 4096;
/// Plumas de la `DrawInfo` del Workbench (las de 3.1 con 4 colores).
const PLUMAS_WB: [u16; dri::NUMDRIPENS] = [0, 1, 1, 2, 1, 3, 1, 0, 2, 1, 2, 1];
/// Cuántos `MOUSEMOVE` y repeticiones de tecla sin contestar como mucho, por defecto (`WA_MouseQueue`,
/// `WA_RptQueue`).
const COLA_RATON: u16 = 5;
/// Cuántos mensajes contestados se recogen de una vez como mucho.
const MAX_RECOGIDA: u32 = 4096;
const COLA_REPETICION: u16 = 3;

impl<C: Compositor> CajaGrafica<C> {
    // --- Instalación: la biblioteca y la pantalla de Workbench ---

    pub(crate) fn instalar_intuition<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        let numeros: Vec<u16> = (0..lvo::INTUITION.len() as u16).map(|i| self.numeros.intuition + i).collect();
        let base = ex.crear_biblioteca_nativa("intuition.library", 40, 1, &numeros, ib::TAMANO)?;
        self.intui.base = base;
        self.intui.ceros = ex.reservar(8, memf::CLEAR)?;
        self.intui.glifos = Self::tabla_glifos();
        self.crear_workbench(ex)?;
        let s = self.intui.wb.dir;
        ex.e32(base + ib::FIRSTSCREEN, s)?;
        ex.e32(base + ib::ACTIVESCREEN, s)?;
        Ok(())
    }

    fn crear_workbench<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        let (an, al) = self.comp.tamano_escritorio();
        let an = an.clamp(64, lienzo::MAX_LADO);
        let mut al = al.clamp(64, lienzo::MAX_LADO);
        while an as u64 * al as u64 > lienzo::MAX_PIXELES {
            al /= 2;
        }
        let cm = self.gfx.cm_wb;
        let s = ex.reservar(sc::TAMANO, memf::CLEAR)?;
        let ta = ex.reservar(textattr::TAMANO, memf::CLEAR)?;
        let nombre = ex.cadena_nueva(fuente::NOMBRE.as_bytes())?;
        ex.e32(ta + textattr::TA_NAME, nombre)?;
        ex.e16(ta + textattr::TA_YSIZE, fuente::ALTO as u16)?;
        ex.e8(ta + textattr::TA_FLAGS, textfont::FPF_ROMFONT)?;
        let titulo = ex.cadena_nueva(b"Workbench Screen")?;

        ex.e16(s + sc::WIDTH, an as u16)?;
        ex.e16(s + sc::HEIGHT, al as u16)?;
        ex.e16(s + sc::FLAGS, sc::WBENCHSCREEN | sc::PUBLICSCREEN | sc::SHOWTITLE | sc::SCREENHIRES)?;
        ex.e32(s + sc::TITLE, titulo)?;
        ex.e32(s + sc::DEFAULTTITLE, titulo)?;
        ex.e8(s + sc::BARHEIGHT, (fuente::ALTO + 2) as u8)?;
        ex.e8(s + sc::BARVBORDER, 1)?;
        ex.e8(s + sc::BARHBORDER, 5)?;
        ex.e8(s + sc::MENUVBORDER, 2)?;
        ex.e8(s + sc::MENUHBORDER, 4)?;
        ex.e32(s + sc::FONT, ta)?;
        ex.e8(s + sc::DETAILPEN, PLUMAS_WB[dri::DETAILPEN] as u8)?;
        ex.e8(s + sc::BLOCKPEN, PLUMAS_WB[dri::BLOCKPEN] as u8)?;
        // ViewPort, con el ColorMap del Workbench.
        let vp = s + sc::VIEWPORT;
        ex.e32(vp + viewport::COLORMAP, cm)?;
        ex.e16(vp + viewport::DWIDTH, an as u16)?;
        ex.e16(vp + viewport::DHEIGHT, al as u16)?;
        ex.e16(vp + viewport::MODES, viewport::HIRES)?;
        ex.e32(cm + colormap::CM_VP, vp)?;
        // BitMap nativo de la pantalla y su RastPort.
        self.nuevo_nativo(ex, s + sc::BITMAP, an, al, cm)?;
        self.init_rast_port(ex, s + sc::RASTPORT)?;
        ex.e32(s + sc::RASTPORT + rp_::BITMAP, s + sc::BITMAP)?;
        // DrawInfo, con sus plumas acabadas en ~0.
        let di = ex.reservar(dri::TAMANO, memf::CLEAR)?;
        let plumas = ex.reservar((dri::NUMDRIPENS as u32 + 1) * 2, memf::CLEAR)?;
        for (i, p) in PLUMAS_WB.iter().enumerate() {
            ex.e16(plumas + 2 * i as u32, *p)?;
        }
        ex.e16(plumas + 2 * dri::NUMDRIPENS as u32, 0xFFFF)?;
        ex.e16(di + dri::DRI_VERSION, dri::VERSION)?;
        ex.e16(di + dri::DRI_NUMPENS, dri::NUMDRIPENS as u16)?;
        ex.e32(di + dri::DRI_PENS, plumas)?;
        ex.e32(di + dri::DRI_FONT, self.gfx.topaz)?;
        ex.e16(di + dri::DRI_DEPTH, 8)?;
        ex.e16(di + dri::DRI_RESOLUTION, 44)?;
        ex.e16(di + dri::DRI_RESOLUTION + 2, 44)?;
        ex.e32(di + dri::DRI_FLAGS, dri::DRIF_NEWLOOK)?;
        self.intui.wb = Pantalla {
            dir: s,
            ancho: an,
            alto: al,
            cm,
            drawinfo: di,
            bloqueos: 0,
            plumas: PLUMAS_WB,
            modo: crate::modos::workbench(),
            tarea: 0,
            bloques: Vec::new(),
        };
        self.bordes_pantalla(ex)?;
        Ok(())
    }

    /// Los bordes de las ventanas que dice la pantalla (`WBorTop`, `WBorLeft`...): los de 3.x, o 0 si los programas de la
    /// caja van "como DashOS" con la barra de DashOS (sus ventanas no llevan bordes de intuition). Lo que vale es el
    /// aspecto de la caja (sin la excepción de ningún programa: la pantalla es de todos). Se pone otra vez al cambiar el
    /// ajuste; las ventanas abiertas no cambian.
    pub(crate) fn bordes_pantalla<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        let s = self.intui.wb.dir;
        if s == 0 {
            return Ok(());
        }
        let dashos = self.comp.pone_barra() && self.comp.tema_amiga("").is_some();
        let (arriba, izq, der, abajo) = if dashos { (0, 0, 0, 0) } else { (2, 4, 4, 2) };
        ex.e8(s + sc::WBORTOP, arriba)?;
        ex.e8(s + sc::WBORLEFT, izq)?;
        ex.e8(s + sc::WBORRIGHT, der)?;
        ex.e8(s + sc::WBORBOTTOM, abajo)?;
        Ok(())
    }

    // --- Pantallas ---

    /// `LockPubScreen`: el Workbench con el nombre 0 o "Workbench"; 0 con otro nombre.
    pub fn lock_pub_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, nombre: u32) -> Result<u32, Fallo> {
        if nombre != 0 && !ex.cadena(nombre)?.eq_ignore_ascii_case(b"Workbench") {
            return Ok(0);
        }
        self.intui.wb.bloqueos += 1;
        Ok(self.intui.wb.dir)
    }

    /// `UnlockPubScreen`.
    pub fn unlock_pub_screen<M: Memoria>(&mut self, ex: &mut Exec<M>, nombre: u32, s: u32) -> Result<(), Fallo> {
        let s = if s != 0 { s } else { self.lock_nombre(ex, nombre)? };
        if s != self.intui.wb.dir {
            return Err(Fallo::PantallaDesconocida { dir: s });
        }
        if self.intui.wb.bloqueos == 0 {
            return Err(Fallo::DesbloqueoDeMas { dir: s });
        }
        self.intui.wb.bloqueos -= 1;
        Ok(())
    }

    fn lock_nombre<M: Memoria>(&self, ex: &Exec<M>, nombre: u32) -> Result<u32, Fallo> {
        if nombre == 0 || ex.cadena(nombre)?.eq_ignore_ascii_case(b"Workbench") { Ok(self.intui.wb.dir) } else { Ok(0) }
    }

    /// Cuántas veces está bloqueado el Workbench.
    pub fn bloqueos_workbench(&self) -> u32 {
        self.intui.wb.bloqueos
    }

    /// `GetScreenData`: copia la `struct Screen` (hasta `largo` bytes). Solo el Workbench.
    pub fn get_screen_data<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        buf: u32,
        largo: u32,
        tipo: u32,
        s: u32,
    ) -> Result<u32, Fallo> {
        let wb = self.intui.wb.dir;
        if tipo != sc::TIPO_WBENCH && s != wb {
            return Err(Fallo::PantallaDesconocida { dir: s });
        }
        let n = (largo & 0xFFFF).min(sc::TAMANO);
        let datos = ex.bytes(wb, n)?;
        ex.escribir_bytes(buf, &datos)?;
        Ok(1)
    }

    /// `GetScreenDrawInfo` (la de la pantalla; es suya, no hay que soltarla).
    pub fn get_screen_draw_info(&mut self, s: u32) -> Result<u32, Fallo> {
        self.pantalla(s).map(|p| p.drawinfo).ok_or(Fallo::PantallaDesconocida { dir: s })
    }

    // --- Abrir ventanas ---

    fn leer_new_window<M: Memoria>(&self, ex: &Exec<M>, n: u32, p: &mut Peticion) -> Result<u32, Fallo> {
        ex.tramo(n, nw::TAMANO)?;
        p.izq = ex.l16(n + nw::LEFTEDGE)? as i16 as i32;
        p.arr = ex.l16(n + nw::TOPEDGE)? as i16 as i32;
        p.ancho = ex.l16(n + nw::WIDTH)? as i16 as i32;
        p.alto = ex.l16(n + nw::HEIGHT)? as i16 as i32;
        p.detalle = ex.l8(n + nw::DETAILPEN)?;
        p.gadgets = ex.l32(n + nw::FIRSTGADGET)?;
        p.bloque = ex.l8(n + nw::BLOCKPEN)?;
        p.idcmp = ex.l32(n + nw::IDCMPFLAGS)?;
        p.flags = ex.l32(n + nw::FLAGS)?;
        p.titulo = ex.l32(n + nw::TITLE)?;
        p.minimo = (ex.l16(n + nw::MINWIDTH)? as i16 as i32, ex.l16(n + nw::MINHEIGHT)? as i16 as i32);
        p.maximo = (ex.l16(n + nw::MAXWIDTH)? as i32, ex.l16(n + nw::MAXHEIGHT)? as i32);
        let tipo = ex.l16(n + nw::TYPE)?;
        if tipo == tipo_pantalla::CUSTOMSCREEN {
            p.pantalla = Some(ex.l32(n + nw::SCREEN)?);
        }
        if p.flags & wflg::NW_EXTENDED != 0 { ex.l32(n + nw::EXTENSION) } else { Ok(0) }
    }

    fn leer_etiquetas<M: Memoria>(&self, ex: &Exec<M>, lista: u32, p: &mut Peticion) -> Result<(), Fallo> {
        let mut t = lista;
        let mut vistas = 0;
        while t != 0 {
            vistas += 1;
            if vistas > MAX_ETIQUETAS {
                return Err(Fallo::EtiquetasMalas { dir: lista });
            }
            let t0 = puntero(t)?;
            let (etiqueta, dato) = (ex.l32(t0 + tag::TI_TAG)?, ex.l32(t0 + tag::TI_DATA)?);
            let si = |p: &mut Peticion, f: u32| {
                if dato != 0 { p.flags |= f } else { p.flags &= !f }
            };
            match etiqueta {
                tag::TAG_DONE => break,
                tag::TAG_MORE => {
                    t = dato;
                    continue;
                }
                tag::TAG_SKIP => {
                    t = t.wrapping_add(tag::TAMANO * (dato.min(MAX_ETIQUETAS) + 1));
                    continue;
                }
                wa::LEFT => p.izq = dato as i32,
                wa::TOP => p.arr = dato as i32,
                wa::WIDTH => p.ancho = dato as i32,
                wa::HEIGHT => p.alto = dato as i32,
                wa::DETAILPEN => p.detalle = dato as u8,
                wa::BLOCKPEN => p.bloque = dato as u8,
                wa::IDCMP => p.idcmp = dato,
                wa::FLAGS => p.flags = dato,
                wa::TITLE => p.titulo = dato,
                wa::SCREENTITLE => p.titulo_pantalla = dato,
                wa::CUSTOMSCREEN | wa::PUBSCREEN => p.pantalla = Some(dato),
                wa::PUBSCREENNAME => p.nombre_pub = Some(dato),
                wa::GADGETS => p.gadgets = dato,
                wa::PUBSCREENFALLBACK => p.respaldo = dato != 0,
                wa::MINWIDTH => p.minimo.0 = dato as i32,
                wa::MINHEIGHT => p.minimo.1 = dato as i32,
                wa::MAXWIDTH => p.maximo.0 = dato as u16 as i32,
                wa::MAXHEIGHT => p.maximo.1 = dato as u16 as i32,
                wa::INNERWIDTH => p.interior.0 = Some(dato as i32),
                wa::INNERHEIGHT => p.interior.1 = Some(dato as i32),
                wa::MOUSEQUEUE => p.cola_raton = dato as u16,
                wa::RPTQUEUE => p.cola_repeticion = dato as u16,
                wa::SIZEGADGET => si(p, wflg::SIZEGADGET),
                wa::DRAGBAR => si(p, wflg::DRAGBAR),
                wa::DEPTHGADGET => si(p, wflg::DEPTHGADGET),
                wa::CLOSEGADGET => si(p, wflg::CLOSEGADGET),
                wa::BACKDROP => si(p, wflg::BACKDROP),
                wa::REPORTMOUSE => si(p, wflg::REPORTMOUSE),
                wa::NOCAREREFRESH => si(p, wflg::NOCAREREFRESH),
                wa::BORDERLESS => si(p, wflg::BORDERLESS),
                wa::ACTIVATE => si(p, wflg::ACTIVATE),
                wa::RMBTRAP => si(p, wflg::RMBTRAP),
                wa::WBENCHWINDOW => si(p, wflg::WBENCHWINDOW),
                wa::SIZEBRIGHT => si(p, wflg::SIZEBRIGHT),
                wa::SIZEBBOTTOM => si(p, wflg::SIZEBBOTTOM),
                wa::GIMMEZEROZERO => si(p, wflg::GIMMEZEROZERO),
                wa::NEWLOOKMENUS => si(p, wflg::NEWLOOKMENUS),
                wa::SIMPLEREFRESH if dato != 0 => p.flags = (p.flags & !wflg::REFRESHBITS) | wflg::SIMPLE_REFRESH,
                wa::SMARTREFRESH if dato != 0 => p.flags &= !wflg::REFRESHBITS,
                _ => {}
            }
            t = t.wrapping_add(tag::TAMANO);
        }
        Ok(())
    }

    /// `OpenWindow` (con `nw`) y `OpenWindowTagList` (con `nw` o 0, y `etiquetas`). Devuelve la ventana, o 0
    /// si no se puede: pantalla que no es el Workbench, tamaño imposible, sin memoria o sin compositor.
    pub fn open_window_tag_list<M: Memoria>(&mut self, ex: &mut Exec<M>, n: u32, etiquetas: u32) -> Result<u32, Fallo> {
        let mut p = Peticion {
            izq: 0,
            arr: 0,
            ancho: 0,
            alto: 0,
            detalle: 0xFF,
            bloque: 0xFF,
            idcmp: 0,
            flags: 0,
            titulo: 0,
            titulo_pantalla: 0,
            pantalla: None,
            nombre_pub: None,
            respaldo: false,
            minimo: (0, 0),
            maximo: (0, 0),
            interior: (None, None),
            cola_raton: COLA_RATON,
            cola_repeticion: COLA_REPETICION,
            gadgets: 0,
        };
        let mut lista = etiquetas;
        if n != 0 {
            let ext = self.leer_new_window(ex, n, &mut p)?;
            if lista == 0 {
                lista = ext;
            }
        }
        self.leer_etiquetas(ex, lista, &mut p)?;
        // La pantalla: la de Workbench o una propia.
        if let Some(s) = p.pantalla
            && s != 0
            && self.pantalla(s).is_none()
        {
            return Err(Fallo::PantallaDesconocida { dir: s });
        }
        if let Some(nombre) = p.nombre_pub
            && self.lock_nombre(ex, nombre)? == 0
            && !p.respaldo
        {
            return Err(Fallo::PantallaDesconocida { dir: nombre });
        }
        self.abrir(ex, p)
    }

    fn abrir<M: Memoria>(&mut self, ex: &mut Exec<M>, mut p: Peticion) -> Result<u32, Fallo> {
        let wb = p.pantalla.filter(|&s| s != 0 && self.pantalla(s).is_some()).unwrap_or(self.intui.wb.dir);
        let (san, sal, cm_pantalla, plumas_pantalla, formato) = {
            let pa = self.pantalla(wb).unwrap_or(&self.intui.wb);
            let f = crate::modos::formato_de_id(pa.modo).unwrap_or(crate::chunky::Formato::Argb32);
            (pa.ancho as i32, pa.alto as i32, pa.cm, pa.plumas, f)
        };
        p.flags &= wflg::DEL_PROGRAMA;
        let tiene_barra = p.titulo != 0 || p.flags & (wflg::DRAGBAR | wflg::CLOSEGADGET | wflg::DEPTHGADGET) != 0;
        // S26: el aspecto que le toca (por el programa que la abre).
        let programa = Self::nombre_programa(ex, ex.tarea_actual());
        let aspecto = self.comp.tema_amiga(&programa);
        // "Como DashOS", con un compositor que pone su barra: la ventana lleva la barra de DashOS (si tiene barra y no
        // es de fondo) e intuition no pinta la suya. Sus bordes, los de la pantalla en ese aspecto (`WBor*` a 0): arriba,
        // `WBorTop` + el alto de la letra + 1 si tiene barra, como en 3.x (los programas que colocan sus cosas con
        // `BorderTop` o con `WBorTop + ta_YSize + 1` cuentan con eso), y nada a los lados ni abajo.
        let dashos = aspecto.is_some() && self.comp.pone_barra() && p.flags & wflg::BORDERLESS == 0;
        let bordes = if p.flags & wflg::BORDERLESS != 0 {
            [0, 0, 0, 0]
        } else if dashos {
            [0, if tiene_barra { fuente::ALTO as i32 + 1 } else { 0 }, 0, 0]
        } else {
            let arriba = if tiene_barra { 2 + fuente::ALTO as i32 + 1 } else { 2 };
            let mut b = [4, arriba, 4, 2];
            if p.flags & wflg::SIZEGADGET != 0 {
                if p.flags & wflg::SIZEBBOTTOM != 0 {
                    b[3] = GADGET_TAMANO.1;
                } else {
                    b[2] = GADGET_TAMANO.0;
                }
            }
            b
        };
        if let Some(a) = p.interior.0 {
            p.ancho = a.saturating_add(bordes[0] + bordes[2]);
        }
        if let Some(a) = p.interior.1 {
            p.alto = a.saturating_add(bordes[1] + bordes[3]);
        }
        if p.ancho == 0 {
            p.ancho = san - p.izq.max(0);
        }
        if p.alto == 0 {
            p.alto = sal - p.arr.max(0);
        }
        if p.ancho < 0 || p.alto < 0 {
            return Err(Fallo::TamanoMalo { ancho: p.ancho, alto: p.alto });
        }
        // Que quepa en la pantalla (como `WA_AutoAdjust`): primero se mueve, luego se encoge.
        let min_an = (bordes[0] + bordes[2] + 1).max(1);
        let min_al = (bordes[1] + bordes[3] + 1).max(1);
        p.ancho = p.ancho.clamp(min_an, san.max(min_an));
        p.alto = p.alto.clamp(min_al, sal.max(min_al));
        p.izq = p.izq.clamp(0, (san - p.ancho).max(0));
        p.arr = p.arr.clamp(0, (sal - p.alto).max(0));
        let (an, al) = (p.ancho, p.alto);
        if !lienzo::tamano_valido(an as i64, al as i64) {
            return Err(Fallo::TamanoMalo { ancho: an, alto: al });
        }
        // Límites: 0 es "el tamaño de ahora"; 0xFFFF en el máximo, sin límite (la pantalla).
        let lim = |v: i32, actual: i32, todo: i32| match v {
            0 => actual,
            0xFFFF | -1 => todo,
            v => v,
        };
        let minimo = (lim(p.minimo.0, an, min_an).clamp(min_an, an), lim(p.minimo.1, al, min_al).clamp(min_al, al));
        let maximo = (lim(p.maximo.0, an, san).clamp(an, san.max(an)), lim(p.maximo.1, al, sal).clamp(al, sal.max(al)));

        let titulo = if p.titulo != 0 { ex.cadena(p.titulo)? } else { Vec::new() };
        // Las estructuras en la memoria del 68k.
        let w = ex.reservar(win::TAMANO, memf::CLEAR)?;
        let rp = ex.reservar(rp_::TAMANO, memf::CLEAR)?;
        let capa = ex.reservar(layer::TAMANO, memf::CLEAR)?;
        let bm = ex.reservar(crate::estructuras::bitmap::TAMANO, memf::CLEAR)?;
        let respuesta = ex.reservar(msgport::TAMANO, memf::CLEAR)?;
        ex.e8(respuesta + node::LN_TYPE, nt::MSGPORT)?;
        ex.e8(respuesta + msgport::MP_FLAGS, msgport::PA_IGNORE)?;
        ex.new_list(respuesta + msgport::MP_MSGLIST, nt::MESSAGE)?;
        let puerto = if p.idcmp != 0 {
            match ex.create_msg_port() {
                Ok(d) if d != 0 => d,
                otro => {
                    for (d, l) in [
                        (w, win::TAMANO),
                        (rp, rp_::TAMANO),
                        (capa, layer::TAMANO),
                        (bm, crate::estructuras::bitmap::TAMANO),
                        (respuesta, msgport::TAMANO),
                    ] {
                        ex.soltar(d, l)?;
                    }
                    otro?;
                    return Err(Fallo::SinMemoria);
                }
            }
        } else {
            0
        };
        let cm = cm_pantalla;
        self.nuevo_nativo(ex, bm, an as u32, al as u32, cm)?;
        // Un programa la ve en el formato del modo de su pantalla (`LockBitMap`, cybergraphics).
        let colores = self.gfx.paletas.get(&cm).cloned().unwrap_or_default();
        if let Some(l) = self.gfx.lienzos.get_mut(&bm) {
            l.poner_formato(formato);
            // En una pantalla de 8 bits, la ventana es de plumas: su paleta, la de la pantalla.
            l.poner_lut(&colores);
        }
        // S26: con el aspecto de DashOS, dentro es de la superficie del tema (la paleta de la pantalla no se toca: un
        // programa que borra con la pluma 0 pinta el gris de siempre).
        let fondo = match crate::tema::Elegido::de(aspecto).como().fondo_ventana() {
            Some(f) => {
                if let Some(l) = self.gfx.lienzos.get_mut(&bm) {
                    l.poner_fondo(f);
                }
                f | 0xFF00_0000
            }
            None => self.color(cm, 0) | 0xFF00_0000,
        };
        // "Como DashOS": los textos de nuestras bibliotecas, con la letra de DashOS al ampliar (`nitido.rs`).
        if let Some(l) = self.gfx.lienzos.get_mut(&bm) {
            l.nitido = aspecto.is_some();
            // Entera al compositor la primera vez (sin bordes de intuition, nadie más la daña toda).
            let r = l.rect();
            l.danar(r);
        }
        self.init_rast_port(ex, rp)?;
        ex.e32(rp + rp_::BITMAP, bm)?;
        ex.e32(rp + rp_::LAYER, capa)?;
        ex.e32(capa + layer::RP, rp)?;
        ex.e32(capa + layer::WINDOW, w)?;
        ex.e16(
            capa + layer::FLAGS,
            if p.flags & wflg::SIMPLE_REFRESH != 0 { layer::LAYERSIMPLE } else { layer::LAYERSMART },
        )?;
        let gzz = (p.flags & wflg::GIMMEZEROZERO != 0).then_some(bordes);
        // La capa, como las de layers: su `Layer_Info` (la de la pantalla), una `ClipRect` que la cubre entera (en su
        // búfer nunca está tapada) y una `DamageList` vacía (MUI la mira para saber qué repintar).
        ex.e32(capa + layer::LAYERINFO, wb + sc::LAYERINFO)?;
        let cliprect = ex.reservar(cliprect::TAMANO, memf::CLEAR)?;
        let dano = match self.region_del_sistema(ex) {
            Ok(d) => d,
            Err(e) => {
                ex.soltar(cliprect, cliprect::TAMANO)?;
                return Err(e);
            }
        };
        ex.e32(capa + layer::CLIPRECT, cliprect)?;
        ex.e32(capa + layer::DAMAGELIST, dano)?;
        self.poner_capa(ex, capa, an, al, gzz)?;
        let plumas = plumas_pantalla;
        let detalle = if p.detalle == 0xFF { plumas[dri::DETAILPEN] as u8 } else { p.detalle };
        let bloque = if p.bloque == 0xFF { plumas[dri::BLOCKPEN] as u8 } else { p.bloque };
        ex.e16(w + win::LEFTEDGE, p.izq as u16)?;
        ex.e16(w + win::TOPEDGE, p.arr as u16)?;
        ex.e16(w + win::MINWIDTH, minimo.0 as u16)?;
        ex.e16(w + win::MINHEIGHT, minimo.1 as u16)?;
        ex.e16(w + win::MAXWIDTH, maximo.0 as u16)?;
        ex.e16(w + win::MAXHEIGHT, maximo.1 as u16)?;
        ex.e32(w + win::FLAGS, p.flags & !wflg::NW_EXTENDED)?;
        ex.e32(w + win::TITLE, p.titulo)?;
        ex.e32(w + win::WSCREEN, wb)?;
        ex.e32(w + win::RPORT, rp)?;
        ex.e8(w + win::BORDERLEFT, bordes[0] as u8)?;
        ex.e8(w + win::BORDERTOP, bordes[1] as u8)?;
        ex.e8(w + win::BORDERRIGHT, bordes[2] as u8)?;
        ex.e8(w + win::BORDERBOTTOM, bordes[3] as u8)?;
        ex.e32(w + win::IDCMPFLAGS, p.idcmp)?;
        ex.e32(w + win::USERPORT, puerto)?;
        ex.e32(w + win::WINDOWPORT, respuesta)?;
        ex.e8(w + win::DETAILPEN, detalle)?;
        ex.e8(w + win::BLOCKPEN, bloque)?;
        ex.e32(w + win::SCREENTITLE, p.titulo_pantalla)?;
        ex.e32(w + win::WLAYER, capa)?;
        ex.e32(w + win::IFONT, self.gfx.topaz)?;
        self.escribir_tamano(ex, w, an, al, bordes)?;
        // Delante en la lista de ventanas de la pantalla.
        let primera = ex.l32(wb + sc::FIRSTWINDOW)?;
        ex.e32(w + win::NEXTWINDOW, primera)?;
        ex.e32(wb + sc::FIRSTWINDOW, w)?;

        let activar = p.flags & wflg::ACTIVATE != 0;
        let pedida = NuevaVentana {
            titulo: titulo.iter().map(|&c| c as char).collect(),
            x: p.izq,
            y: p.arr,
            ancho: an as u32,
            alto: al as u32,
            activa: activar,
            sin_borde: p.flags & (wflg::BORDERLESS | wflg::BACKDROP) != 0,
            minimo: (minimo.0 as u32, minimo.1 as u32),
            maximo: (maximo.0 as u32, maximo.1 as u32),
            programa: programa.clone(),
            barra_dashos: dashos && tiene_barra && p.flags & wflg::BACKDROP == 0,
            // La pestaña de la caja va en la barra de intuition, a la izquierda del gadget de profundidad (si no, tapaba
            // el de cerrar, arriba a la izquierda).
            hueco_marca: (!dashos && tiene_barra && bordes[1] > 2).then(|| {
                let derecha = if p.flags & wflg::DEPTHGADGET != 0 { crate::marco::ANCHO_PROF } else { 0 };
                (derecha, bordes[1])
            }),
        };
        self.intui.orden += 1;
        let orden = self.intui.orden;
        let geometria = Geometria { ancho: an, alto: al, flags: p.flags, bordes, barra_dashos: dashos };
        self.intui.ventanas.insert(
            w,
            Ventana {
                rp,
                capa,
                cliprect,
                dano,
                bm,
                pantalla: wb,
                superficie: 0,
                puerto_creado: puerto,
                respuesta,
                pendientes: BTreeMap::new(),
                tarea: ex.tarea_actual(),
                orden,
                titulo,
                programa,
                fondo,
                aspecto,
                activa: false,
                geometria,
                pulsado: None,
                cola_raton: p.cola_raton,
                cola_repeticion: p.cola_repeticion,
                minimo,
                maximo,
                dir: w,
            },
        );
        self.dibujar_marco(ex, w);
        let Some(s) = self.comp.crear_ventana(&pedida) else {
            self.soltar_ventana(ex, w)?;
            return Err(Fallo::SinCompositor);
        };
        if let Some(v) = self.intui.ventanas.get_mut(&w) {
            v.superficie = s;
        }
        self.intui.por_superficie.insert(s, w);
        if activar {
            self.activar(ex, w, true)?;
        }
        if p.gadgets != 0 {
            self.add_g_list(ex, w, p.gadgets, -1, -1)?;
            self.dibujar_lista(ex, w, p.gadgets, -1, false);
        }
        self.presentar_ventana(ex, w);
        // Una ventana de refresco simple nace con todo por pintar (como en el Amiga, donde su capa nueva no guarda nada):
        // su capa queda marcada (`LAYERREFRESH`) y, si lo pide, recibe `IDCMP_REFRESHWINDOW` (MUI pinta al recibirlo).
        self.refresco_inicial(ex, w)?;
        Ok(w)
    }

    /// Si `w` es de refresco simple y le queda algo por pintar (su capa con `LAYERREFRESH`), `IDCMP_REFRESHWINDOW`:
    /// al abrirla y cuando el programa lo pide después con `ModifyIDCMP` (MUI abre sin IDCMP y pone su puerto luego).
    fn refresco_inicial<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32) -> Result<(), Fallo> {
        let fl = ex.l32(w + win::FLAGS)?;
        if fl & wflg::SIMPLE_REFRESH == 0 || fl & wflg::NOCAREREFRESH != 0 {
            return Ok(());
        }
        let v = self.ventana(w)?;
        let (capa, dano) = (v.capa, v.dano);
        let f = ex.l16(capa + layer::FLAGS)?;
        ex.e16(capa + layer::FLAGS, f | layer::LAYERREFRESH)?;
        // Todo por pintar, en coordenadas de la capa.
        let (an, al) = (ex.l16(capa + layer::WIDTH)?, ex.l16(capa + layer::HEIGHT)?);
        if an > 0 && al > 0 {
            self.poner_region_sistema(ex, dano, &[Rect::nuevo(0, 0, an as i32 - 1, al as i32 - 1)])?;
        }
        let ya = self.ventana(w)?.pendientes.values().any(|&c| c == idcmp::REFRESHWINDOW);
        if !ya {
            self.enviar(ex, w, idcmp::REFRESHWINDOW, 0, 0)?;
        }
        Ok(())
    }

    /// Escribe el tamaño de una ventana en sus estructuras (`Window`, `GZZWidth`, `BitMap`).
    fn escribir_tamano<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        w: u32,
        an: i32,
        al: i32,
        b: [i32; 4],
    ) -> Result<(), Fallo> {
        ex.e16(w + win::WIDTH, an as u16)?;
        ex.e16(w + win::HEIGHT, al as u16)?;
        ex.e16(w + win::GZZWIDTH, (an - b[0] - b[2]).max(0) as u16)?;
        ex.e16(w + win::GZZHEIGHT, (al - b[1] - b[3]).max(0) as u16)?;
        Ok(())
    }

    /// Los límites de la capa de una ventana: en la memoria del 68k y en la tabla de recortes. En una ventana
    /// `WFLG_GIMMEZEROZERO` (con sus `bordes`), la capa del `RPort` es la de dentro: su (0, 0) es la esquina de
    /// dentro de los bordes y no se puede pintar fuera (los bordes los dibuja intuition).
    fn poner_capa<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        capa: u32,
        an: i32,
        al: i32,
        gzz: Option<[i32; 4]>,
    ) -> Result<(), Fallo> {
        let r = match gzz {
            Some([bi, ba, bd, bb]) => Rect::nuevo(bi, ba, an - bd - 1, al - bb - 1),
            None => Rect::de_tamano(an as u32, al as u32),
        };
        let b = capa + layer::BOUNDS;
        ex.e16(b + rectangle::MINX, r.x0 as u16)?;
        ex.e16(b + rectangle::MINY, r.y0 as u16)?;
        ex.e16(b + rectangle::MAXX, r.x1 as u16)?;
        ex.e16(b + rectangle::MAXY, r.y1 as u16)?;
        ex.e16(capa + layer::WIDTH, r.ancho() as u16)?;
        ex.e16(capa + layer::HEIGHT, r.alto() as u16)?;
        let cr = ex.l32(capa + layer::CLIPRECT)?;
        if cr != 0 {
            let c = puntero(cr)? + cliprect::BOUNDS;
            ex.e16(c + rectangle::MINX, r.x0 as u16)?;
            ex.e16(c + rectangle::MINY, r.y0 as u16)?;
            ex.e16(c + rectangle::MAXX, r.x1 as u16)?;
            ex.e16(c + rectangle::MAXY, r.y1 as u16)?;
        }
        self.gfx.capas.insert(capa, r);
        if gzz.is_some() {
            self.gfx.origenes.insert(capa, (r.x0, r.y0));
        } else {
            self.gfx.origenes.remove(&capa);
        }
        Ok(())
    }

    /// Manda al compositor lo que ha cambiado de una ventana.
    pub(crate) fn presentar_ventana<M: Memoria>(&mut self, ex: &Exec<M>, w: u32) {
        let Some(v) = self.intui.ventanas.get(&w) else { return };
        let (s, bm) = (v.superficie, v.bm);
        if s == 0 {
            return;
        }
        let Some(l) = self.gfx.lienzos.get_mut(&bm) else { return };
        let (an, al) = (l.ancho(), l.alto());
        // Lo cambiado, de la memoria del 68k a ARGB (sin píxeles o sin memoria para la copia, nada que mandar).
        let Some((d, pix, textos)) = l.presentar(&ex.mem) else { return };
        self.comp.presentar(s, pix, an, al, d, textos);
    }

    /// Manda al compositor lo que ha cambiado en todas las ventanas. La caja lo llama a cada cuadro (o al volver
    /// de las llamadas nativas).
    pub fn presentar<M: Memoria>(&mut self, ex: &Exec<M>) {
        let ws: Vec<u32> = self.intui.ventanas.keys().copied().collect();
        for w in ws {
            self.presentar_ventana(ex, w);
        }
    }

    // --- Cerrar ---

    /// La ventana de `w`, o el fallo de ventana desconocida.
    pub(crate) fn ventana(&self, w: u32) -> Result<&Ventana, Fallo> {
        self.intui.ventanas.get(&w).ok_or(Fallo::VentanaDesconocida { dir: w })
    }

    /// `CloseWindow`. Los mensajes que esperan en su `UserPort` se recogen; los que tiene el programa sin
    /// contestar hacen que la ventana quede pendiente (su memoria y su `WindowPort` siguen vivos) hasta que los
    /// conteste. Cerrar dos veces, o una que no es, da un aviso.
    pub fn close_window<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32) -> Result<(), Fallo> {
        self.ventana(w)?;
        self.gadtools_ventana_cerrada(w);
        self.boopsi_ventana_cerrada(w);
        self.reclamar(ex, w)?;
        let puerto = ex.l32(w + win::USERPORT)?;
        self.quitar_de_puerto(ex, w, puerto)?;
        let creado = self.intui.ventanas[&w].puerto_creado;
        if creado != 0 && creado == puerto {
            ex.delete_msg_port(puerto)?;
        }
        ex.e32(w + win::USERPORT, 0)?;
        // Fuera de la lista de su pantalla.
        let wb = self.intui.ventanas[&w].pantalla;
        let mut ant = wb + sc::FIRSTWINDOW;
        for _ in 0..4096 {
            let x = ex.l32(ant)?;
            if x == 0 || puntero(x).is_err() {
                break;
            }
            if x == w {
                ex.e32(ant, ex.l32(w + win::NEXTWINDOW)?)?;
                break;
            }
            ant = x + win::NEXTWINDOW;
        }
        if ex.l32(self.intui.base + ib::ACTIVEWINDOW)? == w {
            ex.e32(self.intui.base + ib::ACTIVEWINDOW, 0)?;
        }
        let Some(v) = self.intui.ventanas.remove(&w) else { return Ok(()) };
        self.comp.cerrar_ventana(v.superficie);
        self.intui.por_superficie.remove(&v.superficie);
        self.soltar_lienzo(ex, v.bm);
        self.gfx.capas.remove(&v.capa);
        self.gfx.origenes.remove(&v.capa);
        if v.pendientes.is_empty() {
            self.liberar(ex, &v)?;
        } else {
            self.intui.cerradas.push(v);
        }
        Ok(())
    }

    /// Quita de un puerto los mensajes de la ventana `w` que siguen en su cola, y los suelta.
    fn quitar_de_puerto<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, puerto: u32) -> Result<(), Fallo> {
        if puerto == 0 {
            return Ok(());
        }
        let nodos = ex.nodos(puntero(puerto)? + msgport::MP_MSGLIST)?;
        for m in nodos {
            let es_nuestro = self.intui.ventanas.get(&w).is_some_and(|v| v.pendientes.contains_key(&m));
            if es_nuestro {
                ex.remove(m)?;
                if let Some(v) = self.intui.ventanas.get_mut(&w) {
                    v.pendientes.remove(&m);
                }
                ex.soltar(m, crate::estructuras::intuimessage::TAMANO_EXT)?;
                self.soltar_adjunto(ex, m);
            }
        }
        Ok(())
    }

    /// Lo que se suelta al final de una ventana.
    fn liberar<M: Memoria>(&mut self, ex: &mut Exec<M>, v: &Ventana) -> Result<(), Fallo> {
        self.soltar_dano(ex, v.dano);
        ex.soltar(v.cliprect, cliprect::TAMANO)?;
        ex.soltar(v.respuesta, msgport::TAMANO)?;
        ex.soltar(v.rp, rp_::TAMANO)?;
        ex.soltar(v.capa, layer::TAMANO)?;
        ex.soltar(v.bm, crate::estructuras::bitmap::TAMANO)?;
        ex.soltar(v.dir, win::TAMANO)
    }

    /// Deshace una ventana a medio abrir.
    fn soltar_ventana<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32) -> Result<(), Fallo> {
        let wb = self.intui.ventanas.get(&w).map_or(self.intui.wb.dir, |v| v.pantalla);
        ex.e32(wb + sc::FIRSTWINDOW, ex.l32(w + win::NEXTWINDOW)?)?;
        let Some(v) = self.intui.ventanas.remove(&w) else { return Ok(()) };
        if v.puerto_creado != 0 {
            ex.delete_msg_port(v.puerto_creado)?;
        }
        self.soltar_lienzo(ex, v.bm);
        self.gfx.capas.remove(&v.capa);
        self.gfx.origenes.remove(&v.capa);
        self.liberar(ex, &v)
    }

    /// Recupera los mensajes contestados de una ventana (los que han vuelto a su `WindowPort`).
    pub(crate) fn reclamar<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32) -> Result<(), Fallo> {
        let Some(v) = self.intui.ventanas.get(&w) else { return Ok(()) };
        let r = v.respuesta;
        // Con tope: un puerto estropeado (o que es a la vez el `UserPort`) no debe dejar a la caja dando vueltas.
        for _ in 0..MAX_RECOGIDA {
            let m = ex.get_msg(r)?;
            if m == 0 {
                break;
            }
            let nuestro = self.intui.ventanas.get_mut(&w).and_then(|v| v.pendientes.remove(&m)).is_some();
            if nuestro {
                ex.soltar(m, crate::estructuras::intuimessage::TAMANO_EXT)?;
                self.soltar_adjunto(ex, m);
            } else {
                self.avisar(ex, "ReplyMsg", Fallo::Sospechoso { dir: m });
            }
        }
        Ok(())
    }

    /// Recupera los mensajes contestados de todas las ventanas y acaba las cerradas que ya no esperan nada.
    pub fn reclamar_todo<M: Memoria>(&mut self, ex: &mut Exec<M>) {
        let ws: Vec<u32> = self.intui.ventanas.keys().copied().collect();
        for w in ws {
            if let Err(f) = self.reclamar(ex, w) {
                self.avisar(ex, "CloseWindow", f);
            }
        }
        let cerradas = core::mem::take(&mut self.intui.cerradas);
        for mut v in cerradas {
            for _ in 0..MAX_RECOGIDA {
                let Ok(m) = ex.get_msg(v.respuesta) else { break };
                if m == 0 {
                    break;
                }
                if v.pendientes.remove(&m).is_some() {
                    let _ = ex.soltar(m, crate::estructuras::intuimessage::TAMANO_EXT);
                }
            }
            if v.pendientes.is_empty() {
                if let Err(f) = self.liberar(ex, &v) {
                    self.avisar(ex, "CloseWindow", f);
                }
            } else {
                self.intui.cerradas.push(v);
            }
        }
    }

    /// Cierra las ventanas de una tarea que se va a matar. La caja lo llama antes de `Exec::matar_tarea`.
    pub fn tarea_muerta<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32) {
        self.requesters_tarea_muerta(ex, t);
        let ws: Vec<u32> = self.intui.ventanas.iter().filter(|(_, v)| v.tarea == t).map(|(w, _)| *w).collect();
        for w in ws {
            if let Err(f) = self.close_window(ex, w) {
                self.avisar(ex, "CloseWindow", f);
            }
        }
        self.gadtools_tarea_muerta(t);
        self.regiones_tarea_muerta(t);
        self.capas_tarea_muerta(ex, t);
        self.boopsi_tarea_muerta(ex, t);
        self.cgx_tarea_muerta(ex, t);
        self.pantallas_tarea_muerta(ex, t);
        // Lo que esa tarea tenía sin contestar ya no volverá.
        let cerradas = core::mem::take(&mut self.intui.cerradas);
        for v in cerradas {
            if v.tarea == t {
                for &m in v.pendientes.keys() {
                    let _ = ex.soltar(m, crate::estructuras::intuimessage::TAMANO_EXT);
                    self.soltar_adjunto(ex, m);
                }
                let _ = self.liberar(ex, &v);
            } else {
                self.intui.cerradas.push(v);
            }
        }
    }

    // --- Lo demás de las ventanas ---

    /// `RefreshWindowFrame`.
    pub fn refresh_window_frame<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32) -> Result<(), Fallo> {
        self.ventana(w)?;
        self.dibujar_marco(ex, w);
        self.presentar_ventana(ex, w);
        Ok(())
    }

    /// `SetWindowTitles`: -1 deja el que había.
    pub fn set_window_titles<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        w: u32,
        titulo: u32,
        pantalla: u32,
    ) -> Result<(), Fallo> {
        self.ventana(w)?;
        if pantalla != u32::MAX {
            ex.e32(w + win::SCREENTITLE, pantalla)?;
        }
        if titulo != u32::MAX {
            let t = if titulo != 0 { ex.cadena(titulo)? } else { Vec::new() };
            ex.e32(w + win::TITLE, titulo)?;
            let texto: alloc::string::String = t.iter().map(|&c| c as char).collect();
            if let Some(v) = self.intui.ventanas.get_mut(&w) {
                v.titulo = t;
                let s = v.superficie;
                self.comp.poner_titulo(s, &texto);
            }
            self.refresh_window_frame(ex, w)?;
        }
        Ok(())
    }

    /// `ModifyIDCMP`: con 0 se va el `UserPort` (si lo creó intuition); con algo y sin puerto, se crea uno de la
    /// tarea que llama. Devuelve TRUE, o FALSE si no pudo crear el puerto.
    pub fn modify_idcmp<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, flags: u32) -> Result<u32, Fallo> {
        self.ventana(w)?;
        let puerto = ex.l32(w + win::USERPORT)?;
        let creado = self.intui.ventanas[&w].puerto_creado;
        if flags == 0 {
            if puerto != 0 && puerto == creado {
                self.quitar_de_puerto(ex, w, puerto)?;
                ex.delete_msg_port(puerto)?;
                ex.e32(w + win::USERPORT, 0)?;
            }
            if let Some(v) = self.intui.ventanas.get_mut(&w) {
                v.puerto_creado = 0;
            }
        } else if puerto == 0 {
            let p = ex.create_msg_port()?;
            if p == 0 {
                return Ok(0);
            }
            ex.e32(w + win::USERPORT, p)?;
            if let Some(v) = self.intui.ventanas.get_mut(&w) {
                v.puerto_creado = p;
            }
        } else if puerto != creado
            && let Some(v) = self.intui.ventanas.get_mut(&w)
        {
            // El programa ha puesto su propio puerto (compartido): ese no lo borra intuition.
            v.puerto_creado = 0;
        }
        let antes = ex.l32(w + win::IDCMPFLAGS)?;
        ex.e32(w + win::IDCMPFLAGS, flags)?;
        if flags & idcmp::REFRESHWINDOW != 0 && antes & idcmp::REFRESHWINDOW == 0 {
            let capa = self.ventana(w)?.capa;
            if ex.l16(capa + layer::FLAGS)? & layer::LAYERREFRESH != 0 {
                self.refresco_inicial(ex, w)?;
            }
        }
        Ok(1)
    }

    /// Activa o desactiva una ventana: su bandera, `IntuitionBase->ActiveWindow`, el color de su borde y el
    /// mensaje `ACTIVEWINDOW` o `INACTIVEWINDOW`.
    pub(crate) fn activar<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, si: bool) -> Result<bool, Fallo> {
        let antes = self.ventana(w)?.activa;
        if antes == si {
            return Ok(false);
        }
        let mut replan = false;
        if si {
            // Solo una activa.
            let otras: Vec<u32> =
                self.intui.ventanas.iter().filter(|(d, v)| **d != w && v.activa).map(|(d, _)| *d).collect();
            for o in otras {
                replan |= self.activar(ex, o, false)?;
            }
        }
        if let Some(v) = self.intui.ventanas.get_mut(&w) {
            v.activa = si;
        }
        let f = ex.l32(w + win::FLAGS)?;
        ex.e32(w + win::FLAGS, if si { f | wflg::WINDOWACTIVE } else { f & !wflg::WINDOWACTIVE })?;
        let base = self.intui.base;
        if si {
            ex.e32(base + ib::ACTIVEWINDOW, w)?;
        } else if ex.l32(base + ib::ACTIVEWINDOW)? == w {
            ex.e32(base + ib::ACTIVEWINDOW, 0)?;
        }
        self.dibujar_marco(ex, w);
        let clase = if si { idcmp::ACTIVEWINDOW } else { idcmp::INACTIVEWINDOW };
        replan |= self.enviar(ex, w, clase, 0, 0)?;
        Ok(replan)
    }

    /// `ActivateWindow`: lo pide al compositor; `ACTIVEWINDOW` llega cuando el compositor da el foco.
    pub fn activate_window(&mut self, w: u32) -> Result<(), Fallo> {
        let s = self.ventana(w)?.superficie;
        self.comp.activar(s);
        Ok(())
    }

    pub fn window_to_front(&mut self, w: u32) -> Result<(), Fallo> {
        let s = self.ventana(w)?.superficie;
        self.comp.al_frente(s);
        Ok(())
    }

    pub fn window_to_back(&mut self, w: u32) -> Result<(), Fallo> {
        let s = self.ventana(w)?.superficie;
        self.comp.al_fondo(s);
        Ok(())
    }

    /// `MoveWindow` (relativo): lo pide al compositor, que contesta con la posición nueva.
    pub fn move_window<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, dx: i32, dy: i32) -> Result<(), Fallo> {
        let s = self.ventana(w)?.superficie;
        let x = palabra(ex.l16(w + win::LEFTEDGE)? as u32) + dx;
        let y = palabra(ex.l16(w + win::TOPEDGE)? as u32) + dy;
        self.comp.mover(s, x, y);
        Ok(())
    }

    /// `SizeWindow` (relativo), dentro de los límites: el compositor contesta con el tamaño nuevo.
    pub fn size_window(&mut self, w: u32, dx: i32, dy: i32) -> Result<(), Fallo> {
        let v = self.ventana(w)?;
        let an = (v.geometria.ancho + dx).clamp(v.minimo.0, v.maximo.0);
        let al = (v.geometria.alto + dy).clamp(v.minimo.1, v.maximo.1);
        let s = v.superficie;
        self.comp.pedir_tamano(s, an as u32, al as u32);
        Ok(())
    }

    /// `ChangeWindowBox`.
    pub fn change_window_box(&mut self, w: u32, x: i32, y: i32, an: i32, al: i32) -> Result<(), Fallo> {
        let v = self.ventana(w)?;
        let s = v.superficie;
        let an = an.clamp(v.minimo.0, v.maximo.0);
        let al = al.clamp(v.minimo.1, v.maximo.1);
        self.comp.mover(s, x, y);
        self.comp.pedir_tamano(s, an as u32, al as u32);
        Ok(())
    }

    /// `WindowLimits`: 0 deja el que había. FALSE (y nada cambia) si el tamaño de ahora no cabe en los nuevos.
    pub fn window_limits<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        w: u32,
        min_an: i32,
        min_al: i32,
        max_an: i32,
        max_al: i32,
    ) -> Result<u32, Fallo> {
        let v = self.ventana(w)?;
        let (an, al) = (v.geometria.ancho, v.geometria.alto);
        let elegir = |nuevo: i32, viejo: i32| if nuevo == 0 { viejo } else { nuevo };
        let minimo = (elegir(min_an, v.minimo.0), elegir(min_al, v.minimo.1));
        let maximo = (elegir(max_an, v.maximo.0), elegir(max_al, v.maximo.1));
        if minimo.0 > an || minimo.1 > al || maximo.0 < an || maximo.1 < al {
            return Ok(0);
        }
        let s = v.superficie;
        if let Some(v) = self.intui.ventanas.get_mut(&w) {
            v.minimo = minimo;
            v.maximo = maximo;
        }
        ex.e16(w + win::MINWIDTH, minimo.0 as u16)?;
        ex.e16(w + win::MINHEIGHT, minimo.1 as u16)?;
        ex.e16(w + win::MAXWIDTH, maximo.0 as u16)?;
        ex.e16(w + win::MAXHEIGHT, maximo.1 as u16)?;
        self.comp.limites(s, (minimo.0 as u32, minimo.1 as u32), (maximo.0 as u32, maximo.1 as u32));
        Ok(1)
    }

    /// `BeginRefresh` y `EndRefresh`: el búfer de la ventana no pierde nada, así que solo se apaga la marca de
    /// "hay que refrescar" de la capa al acabar.
    pub fn end_refresh<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, completo: bool) -> Result<(), Fallo> {
        let v = self.ventana(w)?;
        if completo {
            let (capa, dano) = (v.capa, v.dano);
            let f = ex.l16(capa + layer::FLAGS)?;
            ex.e16(capa + layer::FLAGS, f & !layer::LAYERREFRESH)?;
            self.poner_region_sistema(ex, dano, &[])?;
        }
        Ok(())
    }

    /// La capa de la ventana activa (0 si no hay): lo que da `WhichLayer`.
    pub(crate) fn capa_activa(&self) -> u32 {
        self.intui.ventanas.values().find(|v| v.activa).map(|v| v.capa).unwrap_or(0)
    }

    /// `ViewPortAddress`.
    pub fn view_port_address(&mut self, w: u32) -> Result<u32, Fallo> {
        Ok(self.ventana(w)?.pantalla + sc::VIEWPORT)
    }

    /// `CurrentTime`.
    pub fn current_time<M: Memoria>(&mut self, ex: &mut Exec<M>, segundos: u32, micros: u32) -> Result<(), Fallo> {
        let (s, m) = self.intui.hora;
        ex.e32(segundos, s)?;
        ex.e32(micros, m)
    }

    /// Cambia el tamaño de una ventana porque lo dice el compositor: el lienzo (lo que había se queda), las
    /// estructuras, la capa, el borde, y los mensajes `NEWSIZE`, `REFRESHWINDOW` y `CHANGEWINDOW`.
    pub(crate) fn nuevo_tamano<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        w: u32,
        an: u32,
        al: u32,
    ) -> Result<bool, Fallo> {
        let v = self.ventana(w)?;
        let an = (an as i32).min(v.maximo.0).max(v.minimo.0);
        let al = (al as i32).min(v.maximo.1).max(v.minimo.1);
        // Lo que pide el compositor (o unos límites de WindowLimits absurdos) no puede dar un lienzo imposible.
        if !crate::lienzo::tamano_valido(an as i64, al as i64) {
            return Err(Fallo::TamanoMalo { ancho: an, alto: al });
        }
        let g = v.geometria;
        if an == g.ancho && al == g.alto {
            return Ok(false);
        }
        let (bm, capa) = (v.bm, v.capa);
        // Lo de dentro, del color con el que se abrió (el de BACKGROUNDPEN, o la superficie de DashOS).
        let fondo = v.fondo;
        self.cambiar_tamano_lienzo(ex, bm, an as u32, al as u32);
        if let Some(mut l) = crate::graphics::vista(&mut self.gfx.lienzos, ex, bm) {
            let l = &mut l;
            // Lo que era borde abajo y a la derecha queda dentro: se borra.
            let [_, _, bd, bb] = g.bordes;
            for y in 0..al {
                for x in 0..an {
                    if (x >= g.ancho - bd && x < g.ancho) || (y >= g.alto - bb && y < g.alto) {
                        l.poner(x, y, fondo);
                    }
                }
            }
        }
        ex.e16(bm + crate::estructuras::bitmap::BYTESPERROW, (an as u32).div_ceil(16) as u16 * 2)?;
        ex.e16(bm + crate::estructuras::bitmap::ROWS, al as u16)?;
        let bordes = g.bordes;
        let gzz = (ex.l32(w + win::FLAGS)? & wflg::GIMMEZEROZERO != 0).then_some(bordes);
        self.poner_capa(ex, capa, an, al, gzz)?;
        self.escribir_tamano(ex, w, an, al, bordes)?;
        let simple = ex.l32(w + win::FLAGS)? & wflg::SIMPLE_REFRESH != 0;
        if let Some(v) = self.intui.ventanas.get_mut(&w) {
            v.geometria.ancho = an;
            v.geometria.alto = al;
        }
        self.dibujar_marco(ex, w);
        // Los gadgets BOOPSI relativos (el layout de una ventana de ReAction) se colocan otra vez.
        if self.tiene_boopsi(ex, w) {
            self.apuntar(crate::boopsi::Pendiente::Colocar { w });
        }
        let mut replan = self.enviar(ex, w, idcmp::NEWSIZE, 0, 0)?;
        if ex.l32(w + win::FLAGS)? & wflg::NOCAREREFRESH == 0 {
            if simple {
                // Toda la capa por pintar (el programa se recoloca con NEWSIZE y lo repinta todo).
                let f = ex.l16(capa + layer::FLAGS)?;
                ex.e16(capa + layer::FLAGS, f | layer::LAYERREFRESH)?;
                let dano = self.ventana(w)?.dano;
                let r = self.gfx.capas.get(&capa).copied().unwrap_or(Rect::de_tamano(an as u32, al as u32));
                self.poner_region_sistema(ex, dano, &[Rect::nuevo(0, 0, r.ancho() as i32 - 1, r.alto() as i32 - 1)])?;
            }
            replan |= self.enviar(ex, w, idcmp::REFRESHWINDOW, 0, 0)?;
        }
        replan |= self.enviar(ex, w, idcmp::CHANGEWINDOW, 0, 0)?;
        Ok(replan)
    }
}
