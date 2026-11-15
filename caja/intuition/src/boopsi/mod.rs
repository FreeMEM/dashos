//! BOOPSI: las clases y objetos de intuition (V36-V47): `MakeClass`, `AddClass`, `RemoveClass`, `FreeClass`,
//! `NewObjectA`, `DisposeObject`, `SetAttrsA`, `GetAttr`, `SetGadgetAttrsA`, `NextObject`, `DoGadgetMethodA`, las
//! clases de sistema (`sistema.rs`) y las de ReAction (`reaction/`).
//!
//! Escrito desde el RKRM (Libraries, "BOOPSI: Object-Oriented Intuition"), los autodocs de intuition.library, de
//! amiga.lib (`DoMethodA`, `DoSuperMethodA`) y de cada clase de ReAction del NDK 3.2, y los includes del NDK como
//! especificación de la interfaz. No se ha leído código de ninguna implementación de BOOPSI ni de ReAction.
//!
//! **Cómo va.** Cada clase es una `struct IClass` de verdad en la memoria del 68k, con su gancho despachador
//! (`cl_Dispatcher`): `DoMethod` de amiga.lib salta a su `h_Entry` sin pasar por intuition. Las clases nativas tienen
//! todas el mismo `h_Entry`: un trozo de 68k con `$A0FF n; RTS` que entra aquí (`despachar`), y la clase va en A0. Una
//! clase de usuario tiene su despachador en 68k: cuando una clase nativa le manda un método (un `OM_UPDATE` por
//! `ICA_TARGET`, el `OM_NEW` de `NewObjectA`, el `GM_RENDER` de un gadget suyo), se ejecuta su código con
//! `Puente::ejecutar_68k`, y él puede volver a llamar a intuition (`DoSuperMethodA`): la caja abre otra llamada
//! nativa dentro.
//!
//! **Sin referencias a través del 68k.** Por eso nada de esto es un método de `CajaGrafica` con `&mut self`: todo
//! recibe el `Puente`, pide `partes()` cuando lo necesita y suelta la referencia antes de ejecutar 68k (el comprobador
//! de préstamos lo asegura: las dos cosas piden `&mut` al puente). Lo que pasa en intuition por eventos (un clic en un
//! gadget BOOPSI, repintar uno) no se puede hacer desde `CajaGrafica::atender`, que tiene `&mut self`: se apunta en
//! una cola y la caja la vacía después (`atender`), con el puente.
//!
//! **Lo malo.** Una clase que no existe, un objeto que no es un objeto, un método desconocido: aviso y 0. Un
//! despachador 68k que no vuelve: lo corta la caja (el programa acaba; aquí llega `Corte68k::Acabado` y se deshace
//! todo sin tocar nada más). Uno que se llama a sí mismo por intuition sin fin: tope de niveles (`MAX_NIVELES`).
//! Interconexiones en círculo (A notifica a B y B a A): cada objeto, mientras notifica, no atiende más
//! notificaciones (lo que en icclass son `ICM_SETLOOP` y `ICM_CHECKLOOP`).

pub mod cte;
mod pintar;
pub(crate) mod reaction;
pub(crate) mod sistema;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Memoria, Registros};

use crate::acceso::Acceso;
use crate::lienzo::Rect;
use crate::{CajaGrafica, Compositor, Fallo};
use cte::*;

/// Lo que la caja da a BOOPSI: sus bibliotecas y cómo ejecutar código 68k.
pub trait Puente {
    type M: Memoria;
    type C: Compositor;
    /// Exec y graphics/intuition. La referencia no puede vivir a través de `ejecutar_68k`.
    fn partes(&mut self) -> (&mut Exec<Self::M>, &mut CajaGrafica<Self::C>);
    /// Ejecuta código 68k desde `dir` con `regs` hasta su `RTS` (A7 = 0: la pila de la llamada nativa en curso) y
    /// devuelve los registros al volver.
    fn ejecutar_68k(&mut self, dir: u32, regs: &Registros) -> Result<Registros, Corte68k>;
}

/// Por qué no volvió el 68k.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corte68k {
    /// El programa acabó dentro (o la caja lo cortó): hay que deshacerlo todo sin hacer nada más.
    Acabado,
    /// No se pudo ejecutar (demasiados niveles, sin pila): sigue como si hubiera dado 0.
    NoSePudo,
}

/// Lo que puede cortar un método: un fallo (se apunta y el método da 0) o que el programa acabó dentro del 68k.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Fallo(Fallo),
    Cortado,
}

impl From<Fallo> for Error {
    fn from(f: Fallo) -> Error {
        Error::Fallo(f)
    }
}

impl From<caja_exec::Fallo> for Error {
    fn from(f: caja_exec::Fallo) -> Error {
        Error::Fallo(f.into())
    }
}

pub(crate) type R<T> = Result<T, Error>;

/// Cuántos métodos anidados (de nativo a nativo o a 68k) antes de cortar: más es un bucle.
pub const MAX_NIVELES: u32 = 48;
/// Lo más que se atiende de la cola por vuelta (un repintado que pide otro, sin fin).
const MAX_COLA: usize = 512;
/// Tope de hijos de un grupo o de un layout.
pub(crate) const MAX_HIJOS: usize = 4096;

/// Las clases nativas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Nativa {
    Root,
    Ic,
    Model,
    Image,
    FrameI,
    SysI,
    FillRect,
    IText,
    Gadget,
    PropG,
    StrG,
    ButtonG,
    FrButton,
    GroupG,
    // ReAction (reaction/).
    Ventana,
    Layout,
    Boton,
    Casilla,
    Elector,
    Entero,
    Cadena,
    Deslizador,
    Barra,
    Lista,
    Radio,
    Espacio,
    Archivo,
    Etiqueta,
    Bisel,
    Glifo,
}

/// (clase, nombre, superclase, tamaño de sus datos en la memoria del 68k). En orden: la superclase antes.
pub(crate) const SISTEMA: &[(Nativa, &str, Option<Nativa>, u16)] = &[
    (Nativa::Root, "rootclass", None, 0),
    (Nativa::Ic, "icclass", Some(Nativa::Root), 0),
    (Nativa::Model, "modelclass", Some(Nativa::Ic), 0),
    (Nativa::Image, "imageclass", Some(Nativa::Root), image::TAMANO as u16),
    (Nativa::FrameI, "frameiclass", Some(Nativa::Image), 0),
    (Nativa::SysI, "sysiclass", Some(Nativa::Image), 0),
    (Nativa::FillRect, "fillrectclass", Some(Nativa::Image), 0),
    (Nativa::IText, "itexticlass", Some(Nativa::Image), 0),
    (Nativa::Gadget, "gadgetclass", Some(Nativa::Root), TAMANO_GADGET as u16),
    (Nativa::PropG, "propgclass", Some(Nativa::Gadget), 0),
    (Nativa::StrG, "strgclass", Some(Nativa::Gadget), 0),
    (Nativa::ButtonG, "buttongclass", Some(Nativa::Gadget), 0),
    (Nativa::FrButton, "frbuttonclass", Some(Nativa::ButtonG), 0),
    (Nativa::GroupG, "groupgclass", Some(Nativa::Gadget), 0),
    (Nativa::Ventana, "window.class", Some(Nativa::Root), 0),
    (Nativa::Layout, "layout.gadget", Some(Nativa::Gadget), 0),
    (Nativa::Boton, "button.gadget", Some(Nativa::Gadget), 0),
    (Nativa::Casilla, "checkbox.gadget", Some(Nativa::Gadget), 0),
    (Nativa::Elector, "chooser.gadget", Some(Nativa::Gadget), 0),
    (Nativa::Entero, "integer.gadget", Some(Nativa::StrG), 0),
    (Nativa::Cadena, "string.gadget", Some(Nativa::StrG), 0),
    (Nativa::Deslizador, "slider.gadget", Some(Nativa::Gadget), 0),
    (Nativa::Barra, "scroller.gadget", Some(Nativa::Gadget), 0),
    (Nativa::Lista, "listbrowser.gadget", Some(Nativa::Gadget), 0),
    (Nativa::Radio, "radiobutton.gadget", Some(Nativa::Gadget), 0),
    (Nativa::Espacio, "space.gadget", Some(Nativa::Gadget), 0),
    (Nativa::Archivo, "getfile.gadget", Some(Nativa::Gadget), 0),
    (Nativa::Etiqueta, "label.image", Some(Nativa::Image), 0),
    (Nativa::Bisel, "bevel.image", Some(Nativa::Image), 0),
    (Nativa::Glifo, "glyph.image", Some(Nativa::Image), 0),
];

/// Una clase que intuition conoce.
pub(crate) struct Clase {
    /// La nativa que es (las de usuario: `None`).
    pub nativa: Option<Nativa>,
    /// La tarea que la hizo con `MakeClass` (0: del sistema).
    pub tarea: u32,
}

/// Un objeto vivo.
pub(crate) struct Obj {
    /// El bloque de `AllocMem` (el objeto empieza `CABECERA` más allá) y su largo.
    pub bloque: u32,
    pub largo: u32,
    /// Su clase de verdad.
    pub clase: u32,
    pub tarea: u32,
    pub d: Datos,
}

/// Lo nativo de un objeto: lo de las clases de sistema de su cadena y lo de su clase de ReAction.
#[derive(Default)]
pub(crate) struct Datos {
    /// `ICA_TARGET` e `ICA_MAP` (icclass y gadgetclass).
    pub objetivo: u32,
    pub mapa: u32,
    /// Notificando ahora mismo: no atiende más notificaciones (interconexiones en círculo).
    pub bucle: bool,
    /// Los miembros (modelclass, groupgclass) o los hijos (layout).
    pub miembros: Vec<u32>,
    /// El miembro activo (groupgclass, layout).
    pub activo: u32,
    /// El texto de `GA_Text` o de la etiqueta.
    pub texto: Vec<u8>,
    /// Lo numérico de las clases sencillas de ReAction (nivel, mínimo, máximo, elegido, arriba, total...).
    pub min: i32,
    pub max: i32,
    pub valor: i32,
    pub arriba: i32,
    pub total: i32,
    pub visible: i32,
    pub orient: u32,
    /// Una lista de nodos (de exec) o un vector de cadenas.
    pub lista: u32,
    /// Un bloque nuestro en la memoria del 68k (el `IBox` de `SPACE_AreaBox`), que se suelta con el objeto.
    pub caja: u32,
    pub x: Extra,
}

/// Lo propio de cada clase nativa.
#[derive(Default)]
pub(crate) enum Extra {
    #[default]
    Nada,
    Prop(sistema::Prop),
    Cadena(sistema::Cadena),
    Marco(sistema::Marco),
    Sys(u32),
    Relleno,
    Ra(alloc::boxed::Box<reaction::Ra>),
}

/// Lo que se hará fuera de `CajaGrafica::atender` (que no puede ejecutar 68k).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pendiente {
    /// Repintar un gadget BOOPSI de una ventana (`AddGList`, `RefreshGList`, el cambio de tema...).
    Pintar { w: u32, g: u32 },
    /// Colocar los gadgets con `GFLG_RELSPECIAL` y los relativos de una ventana (`GM_LAYOUT`) y repintarlos.
    Colocar { w: u32 },
    /// Un evento para los gadgets BOOPSI de una ventana.
    Entrada { w: u32, e: Entrada },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entrada {
    /// Botón izquierdo abajo en (x, y) de la ventana.
    Pulsar { x: i32, y: i32 },
    Mover { x: i32, y: i32 },
    Soltar { x: i32, y: i32 },
    /// Una tecla (código crudo del Amiga) con su carácter, si tiene.
    Tecla { crudo: u8, ch: Option<u8>, calif: u16 },
}

/// El gadget BOOPSI activo (entre `GM_GOACTIVE` y `GM_GOINACTIVE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Activo {
    pub w: u32,
    pub g: u32,
}

#[derive(Default)]
pub(crate) struct Estado {
    /// El trozo de 68k de los despachadores nativos (`$A0FF n; RTS`).
    pub stub: u32,
    pub clases: BTreeMap<u32, Clase>,
    /// Las nativas por tipo.
    pub nativas: BTreeMap<Nativa, u32>,
    /// La lista pública (`AddClass`), en orden.
    pub publicas: Vec<u32>,
    pub objetos: BTreeMap<u32, Obj>,
    pub niveles: u32,
    pub cola: VecDeque<Pendiente>,
    pub activo: Option<Activo>,
    /// La hoja que terminó (un hijo de un layout): el `IAddress` del `GADGETUP`.
    pub hoja: u32,
    /// El carácter de la tecla que se está mandando (lo que daría `MapRawKey`).
    pub caracter: Option<u8>,
    /// Lo que va con un mensaje IDCMP (la copia de las etiquetas de un `IDCMPUPDATE`): se suelta al volver.
    pub adjuntos: BTreeMap<u32, (u32, u32)>,
    /// Las funciones de las bibliotecas de clases de ReAction: número nativo → (clase, función).
    pub obtener: BTreeMap<u16, (Nativa, &'static str)>,
    /// Los nodos de las listas de ReAction (chooser, listbrowser, radiobutton).
    pub nodos: BTreeMap<u32, reaction::Nodo>,
    /// El número nativo del despachador.
    pub n_despacho: u16,
}

impl<C: Compositor> CajaGrafica<C> {
    /// Crea las clases de sistema y las de ReAction (con sus bibliotecas: `window.class`, `gadgets/layout.gadget`...).
    pub(crate) fn instalar_boopsi<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        let n = self.numeros.boopsi;
        let stub = ex.reservar(8, memf::CLEAR)?;
        ex.e16(stub, 0xA0FF)?;
        ex.e16(stub + 2, n)?;
        ex.e16(stub + 4, 0x4E75)?;
        self.boopsi.stub = stub;
        self.boopsi.n_despacho = n;
        for &(k, nombre, sup, tam) in SISTEMA {
            let sup = sup.and_then(|s| self.boopsi.nativas.get(&s).copied()).unwrap_or(0);
            let cl = ex.reservar(iclass::TAMANO, memf::CLEAR)?;
            let id = ex.cadena_nueva(nombre.as_bytes())?;
            ex.e32(cl + iclass::H_ENTRY, stub)?;
            ex.e32(cl + iclass::SUPER, sup)?;
            ex.e32(cl + iclass::ID, id)?;
            let (off, tam_sup) = if sup != 0 {
                (ex.l16(sup + iclass::INSTOFFSET)?, ex.l16(sup + iclass::INSTSIZE)?)
            } else {
                (0, 0)
            };
            ex.e16(cl + iclass::INSTOFFSET, off + tam_sup)?;
            ex.e16(cl + iclass::INSTSIZE, tam)?;
            ex.e32(cl + iclass::FLAGS, iclass::CLF_INLIST)?;
            if sup != 0 {
                let s = ex.l32(sup + iclass::SUBCLASSCOUNT)?;
                ex.e32(sup + iclass::SUBCLASSCOUNT, s + 1)?;
            }
            self.boopsi.clases.insert(cl, Clase { nativa: Some(k), tarea: 0 });
            self.boopsi.nativas.insert(k, cl);
            self.boopsi.publicas.push(cl);
        }
        // Las bibliotecas de clases de ReAction: lo que abren los programas (OpenLibrary con su versión) y su única
        // función, `XXX_GetClass` (-30).
        let mut num = n + 1;
        for &(k, nombre, funciones) in reaction::BIBLIOTECAS {
            let nums: Vec<u16> = (0..funciones.len() as u16).map(|i| num + i).collect();
            ex.crear_biblioteca_nativa(nombre, 47, 1, &nums, 34)?;
            for (i, &f) in funciones.iter().enumerate() {
                self.boopsi.obtener.insert(num + i as u16, (k, f));
            }
            num += funciones.len() as u16;
        }
        Ok(())
    }

    /// Si el número nativo es de BOOPSI: las funciones de intuition de BOOPSI, el despachador de las clases
    /// nativas y los `GetClass` de ReAction. La caja los llama con `boopsi::llamar` (con su puente), no con `llamar`.
    pub fn es_boopsi(&self, n: u16) -> bool {
        if n == self.boopsi.n_despacho || self.boopsi.obtener.contains_key(&n) {
            return true;
        }
        let i0 = self.numeros.intuition;
        n >= i0 && funcion_boopsi((n - i0) as usize).is_some()
    }

    /// La clase nativa de ese nombre (para las pruebas).
    pub fn clase_publica(&self, nombre: &str) -> Option<u32> {
        SISTEMA.iter().find(|s| s.1 == nombre).and_then(|s| self.boopsi.nativas.get(&s.0).copied())
    }

    /// Cuántos objetos BOOPSI hay vivos.
    pub fn objetos_boopsi(&self) -> usize {
        self.boopsi.objetos.len()
    }

    /// Si hay algo de BOOPSI que hacer fuera (la caja llama a `boopsi::atender`).
    pub fn boopsi_pendiente(&self) -> bool {
        !self.boopsi.cola.is_empty()
    }

    pub(crate) fn es_objeto(&self, o: u32) -> bool {
        self.boopsi.objetos.contains_key(&o)
    }

    pub(crate) fn apuntar(&mut self, p: Pendiente) {
        if self.boopsi.cola.len() < 4 * MAX_COLA && !self.boopsi.cola.contains(&p) {
            self.boopsi.cola.push_back(p);
        }
    }

    /// Lo de BOOPSI de una tarea que acaba: sus clases y sus objetos (su memoria la suelta exec).
    pub(crate) fn boopsi_tarea_muerta<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32) {
        let objs: Vec<u32> = self.boopsi.objetos.iter().filter(|(_, o)| o.tarea == t).map(|(d, _)| *d).collect();
        for o in objs {
            // Su memoria es de la tarea: la suelta exec (y cuenta como no soltada por el programa).
            if let Some(ob) = self.boopsi.objetos.remove(&o) {
                let n = ex.l32(ob.clase + iclass::OBJECTCOUNT).unwrap_or(1);
                let _ = ex.e32(ob.clase + iclass::OBJECTCOUNT, n.saturating_sub(1));
            }
        }
        let nodos: Vec<u32> = self.boopsi.nodos.iter().filter(|(_, n)| n.tarea == t).map(|(d, _)| *d).collect();
        for n in nodos {
            self.boopsi.nodos.remove(&n);
        }
        let clases: Vec<u32> = self.boopsi.clases.iter().filter(|(_, c)| c.tarea == t && t != 0).map(|(d, _)| *d).collect();
        for cl in clases {
            self.boopsi.clases.remove(&cl);
            self.boopsi.publicas.retain(|&c| c != cl);
            let sup = ex.l32(cl + iclass::SUPER).unwrap_or(0);
            if self.boopsi.clases.contains_key(&sup) {
                let s = ex.l32(sup + iclass::SUBCLASSCOUNT).unwrap_or(1);
                let _ = ex.e32(sup + iclass::SUBCLASSCOUNT, s.saturating_sub(1));
            }
        }
        if self.boopsi.activo.is_some_and(|a| !self.intui.ventanas.contains_key(&a.w) || !self.es_objeto(a.g)) {
            self.boopsi.activo = None;
        }
        self.boopsi.cola.retain(|p| match *p {
            Pendiente::Pintar { w, .. } | Pendiente::Colocar { w } | Pendiente::Entrada { w, .. } => {
                self.intui.ventanas.contains_key(&w)
            }
        });
    }

    /// Una ventana se cierra: fuera lo suyo de la cola y el gadget activo.
    pub(crate) fn boopsi_ventana_cerrada(&mut self, w: u32) {
        if self.boopsi.activo.is_some_and(|a| a.w == w) {
            self.boopsi.activo = None;
        }
        self.boopsi.cola.retain(|p| match *p {
            Pendiente::Pintar { w: x, .. } | Pendiente::Colocar { w: x } | Pendiente::Entrada { w: x, .. } => x != w,
        });
    }

    /// Suelta lo que iba con un mensaje IDCMP que ya volvió (o que no volverá).
    pub(crate) fn soltar_adjunto<M: Memoria>(&mut self, ex: &mut Exec<M>, m: u32) {
        if let Some((d, l)) = self.boopsi.adjuntos.remove(&m) {
            let _ = ex.soltar(d, l);
        }
    }

    /// Si `g` es un gadget BOOPSI de verdad (un objeto de una clase de gadgets).
    pub(crate) fn es_gadget_boopsi<M: Memoria>(&self, ex: &Exec<M>, g: u32) -> bool {
        self.es_objeto(g)
            && ex.l16(g.wrapping_add(crate::gadtools::gadget::GADGETTYPE)).unwrap_or(0) & gtyp::GTYPEMASK
                == gtyp::CUSTOMGADGET
    }

    /// Si los eventos del ratón y del teclado de la ventana `w` son de BOOPSI: hay un gadget suyo activo, o un evento
    /// suyo en la cola (el que lo activará).
    pub(crate) fn boopsi_activo_en(&self, w: u32) -> bool {
        self.boopsi.activo.is_some_and(|a| a.w == w)
            || self.boopsi.cola.iter().any(|p| matches!(*p, Pendiente::Entrada { w: x, .. } if x == w))
    }

    /// Si la ventana tiene algún gadget BOOPSI.
    pub(crate) fn tiene_boopsi<M: Memoria>(&self, ex: &Exec<M>, w: u32) -> bool {
        let mut g = ex.l32(w.wrapping_add(crate::estructuras::window::FIRSTGADGET)).unwrap_or(0);
        let mut k = 0;
        while g != 0 && k < 10_000 {
            if self.es_gadget_boopsi(ex, g) {
                return true;
            }
            g = ex.l32(g).unwrap_or(0);
            k += 1;
        }
        false
    }

    /// `SetMenuStrip` y `ClearMenuStrip` desde lo nativo (window.class).
    pub(crate) fn set_menu_strip_nativo<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, menu: u32) -> Result<(), Fallo> {
        self.ventana(w)?;
        ex.e32(w + crate::estructuras::window::MENUSTRIP, menu)?;
        Ok(())
    }

    pub(crate) fn clear_menu_strip_nativo<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32) -> Result<(), Fallo> {
        self.ventana(w)?;
        self.menu_ventana_cerrada(w);
        ex.e32(w + crate::estructuras::window::MENUSTRIP, 0)?;
        Ok(())
    }
}

// --- Las funciones de intuition ---

const NEWOBJECTA: &str = "NewObjectA";
const DISPOSEOBJECT: &str = "DisposeObject";
const SETATTRSA: &str = "SetAttrsA";
const GETATTR: &str = "GetAttr";
const SETGADGETATTRSA: &str = "SetGadgetAttrsA";
const NEXTOBJECT: &str = "NextObject";
const MAKECLASS: &str = "MakeClass";
const ADDCLASS: &str = "AddClass";
const REMOVECLASS: &str = "RemoveClass";
const FREECLASS: &str = "FreeClass";
const DOGADGETMETHODA: &str = "DoGadgetMethodA";

fn funcion_boopsi(fila: usize) -> Option<&'static str> {
    let n = crate::lvo::INTUITION.get(fila)?.0;
    [NEWOBJECTA, DISPOSEOBJECT, SETATTRSA, GETATTR, SETGADGETATTRSA, NEXTOBJECT, MAKECLASS, ADDCLASS, REMOVECLASS, FREECLASS, DOGADGETMETHODA]
        .into_iter()
        .find(|&f| f == n)
}

/// Lo que devuelve `llamar`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hecho {
    /// Hecho (D0 tiene lo que devuelve).
    Seguir,
    /// El programa acabó dentro de un despachador 68k: la caja tiene que cortarlo.
    Acabar,
    /// No es de BOOPSI.
    Ajena,
}

/// Una función nativa de BOOPSI (`es_boopsi`), con los registros de la tarea. Al acabar atiende lo que haya en la
/// cola (los repintados que pidió).
pub fn llamar<P: Puente>(p: &mut P, n: u16, r: &mut Registros) -> Hecho {
    let (despacho, i0, clase_lib) = {
        let (_, gr) = p.partes();
        (gr.boopsi.n_despacho, gr.numeros.intuition, gr.boopsi.obtener.get(&n).copied())
    };
    let res: R<u32> = if n == despacho {
        despachar(p, r.a[0], r.a[2], r.a[1])
    } else if let Some((k, f)) = clase_lib {
        reaction::funcion(p, k, f, r)
    } else {
        let Some(f) = n.checked_sub(i0).and_then(|f| funcion_boopsi(f as usize)) else { return Hecho::Ajena };
        let (d, a) = (r.d, r.a);
        match f {
            NEWOBJECTA => new_object(p, a[0], a[1], a[2]),
            DISPOSEOBJECT => dispose_object(p, a[0]).map(|_| 0),
            SETATTRSA => set_attrs(p, a[0], 0, a[1]),
            GETATTR => get_attr(p, d[0], a[0], a[1]),
            SETGADGETATTRSA => set_attrs(p, a[0], a[1], a[3]),
            NEXTOBJECT => next_object(p, a[0]),
            MAKECLASS => make_class(p, a[0], a[1], a[2], d[0], d[1]),
            ADDCLASS => add_class(p, a[0]).map(|_| 0),
            REMOVECLASS => remove_class(p, a[0]).map(|_| 0),
            FREECLASS => free_class(p, a[0]),
            DOGADGETMETHODA => do_gadget_method(p, a[0], a[1], a[3]),
            _ => Ok(0),
        }
    };
    let res = res.and_then(|v| atender_cola(p).map(|_| v));
    match res {
        Ok(v) => {
            r.d[0] = v;
            Hecho::Seguir
        }
        Err(Error::Fallo(f)) => {
            let (ex, gr) = p.partes();
            gr.avisar(ex, nombre_de(gr, n), f);
            r.d[0] = 0;
            Hecho::Seguir
        }
        Err(Error::Cortado) => {
            r.d[0] = 0;
            Hecho::Acabar
        }
    }
}

fn nombre_de<C: Compositor>(gr: &CajaGrafica<C>, n: u16) -> &'static str {
    if n == gr.boopsi.n_despacho {
        return "(despachador BOOPSI)";
    }
    n.checked_sub(gr.numeros.intuition).and_then(|f| funcion_boopsi(f as usize)).unwrap_or("(GetClass)")
}

/// Lo que la caja llama tras atender al compositor: los eventos y repintados de los gadgets BOOPSI. Devuelve
/// `false` si el programa acabó dentro de un despachador 68k.
pub fn atender<P: Puente>(p: &mut P) -> bool {
    let r = atender_cola(p);
    let (ex, gr) = p.partes();
    gr.presentar(ex);
    r != Err(Error::Cortado)
}

// --- Apoyo ---

pub(crate) fn l32<P: Puente>(p: &mut P, d: u32) -> R<u32> {
    Ok(p.partes().0.l32(d)?)
}
pub(crate) fn l16<P: Puente>(p: &mut P, d: u32) -> R<u16> {
    Ok(p.partes().0.l16(d)?)
}
pub(crate) fn e32<P: Puente>(p: &mut P, d: u32, v: u32) -> R<()> {
    Ok(p.partes().0.e32(d, v)?)
}
pub(crate) fn e16<P: Puente>(p: &mut P, d: u32, v: u16) -> R<()> {
    Ok(p.partes().0.e16(d, v)?)
}

/// Las etiquetas de una lista del 68k (con `TAG_MORE`, `TAG_SKIP`...).
pub(crate) fn etiquetas<P: Puente>(p: &mut P, d: u32) -> R<Vec<(u32, u32)>> {
    if d == 0 {
        return Ok(Vec::new());
    }
    Ok(crate::gadtools::leer_etiquetas(p.partes().0, d)?)
}

pub(crate) fn buscar(e: &[(u32, u32)], t: u32) -> Option<u32> {
    e.iter().rev().find(|(k, _)| *k == t).map(|(_, v)| *v)
}

/// Un mensaje en la memoria del 68k con esas palabras (las parejas de WORD van juntas: `(x << 16) | y`), lo que
/// haga `f` con él, y fuera.
pub(crate) fn con_mensaje<P: Puente, T>(p: &mut P, palabras: &[u32], f: impl FnOnce(&mut P, u32) -> R<T>) -> R<T> {
    let largo = 4 * palabras.len() as u32 + 8;
    let m = p.partes().0.reservar(largo, memf::CLEAR)?;
    for (i, &w) in palabras.iter().enumerate() {
        e32(p, m + 4 * i as u32, w)?;
    }
    let r = f(p, m);
    let _ = p.partes().0.soltar(m, largo);
    r
}

/// Una lista de etiquetas en la memoria del 68k.
pub(crate) fn con_etiquetas<P: Puente, T>(p: &mut P, t: &[(u32, u32)], f: impl FnOnce(&mut P, u32) -> R<T>) -> R<T> {
    let largo = 8 * (t.len() as u32 + 1);
    let d = p.partes().0.reservar(largo, memf::CLEAR)?;
    for (i, &(k, v)) in t.iter().enumerate() {
        e32(p, d + 8 * i as u32, k)?;
        e32(p, d + 8 * i as u32 + 4, v)?;
    }
    let r = f(p, d);
    let _ = p.partes().0.soltar(d, largo);
    r
}

/// La clase de un objeto (`OCLASS`), si es un objeto vivo.
pub(crate) fn clase_de<P: Puente>(p: &mut P, o: u32) -> R<u32> {
    match clase_de_objeto(p, o)? {
        Some(c) => Ok(c),
        None => Err(Fallo::Sospechoso { dir: o }.into()),
    }
}

/// Los datos nativos de un objeto, si vive.
pub(crate) fn datos<P: Puente>(p: &mut P, o: u32) -> Option<&mut Datos> {
    p.partes().1.boopsi.objetos.get_mut(&o).map(|ob| &mut ob.d)
}

/// La clase nativa de `cl` (o la primera nativa de sus superclases, para una de usuario que use el despachador de
/// una nativa).
fn nativa_de<P: Puente>(p: &mut P, cl: u32) -> R<Option<(Nativa, u32)>> {
    let mut c = cl;
    for _ in 0..64 {
        let k = p.partes().1.boopsi.clases.get(&c).map(|x| x.nativa);
        match k {
            Some(Some(n)) => return Ok(Some((n, c))),
            Some(None) => c = l32(p, c + iclass::SUPER)?,
            None => return Ok(None),
        }
    }
    Ok(None)
}

/// Una clase que no hizo `MakeClass` (MUI 5 arma las suyas a mano, con su propia clase raíz: una `struct IClass` con
/// su despachador en 68k y `cl_Super` a 0 en la de arriba): si parece una de verdad (se puede leer entera y su `h_Entry`
/// es una dirección de código par, ella y sus superclases, como mucho 32, sin dar vueltas, hasta una que la caja conoce
/// o hasta `cl_Super` = 0), se apunta como de usuario, del sistema (no se va con ninguna tarea). Intuition no comprueba
/// las clases que le dan; la caja comprueba esto. Devuelve si la conoce (ya o desde ahora).
pub(crate) fn adoptar_clase<P: Puente>(p: &mut P, cl: u32) -> R<bool> {
    let mut cadena = Vec::new();
    let mut c = cl;
    for _ in 0..32 {
        if c == 0 && !cadena.is_empty() {
            break;
        }
        if p.partes().1.boopsi.clases.contains_key(&c) {
            break;
        }
        let ex = p.partes().0;
        if c == 0 || ex.tramo(c, iclass::TAMANO).is_err() {
            return Ok(false);
        }
        let entrada = ex.l32(c + iclass::H_ENTRY)?;
        if entrada == 0 || entrada & 1 != 0 || ex.tramo(entrada, 2).is_err() || cadena.contains(&c) {
            return Ok(false);
        }
        cadena.push(c);
        c = ex.l32(c + iclass::SUPER)?;
    }
    if c != 0 && !p.partes().1.boopsi.clases.contains_key(&c) {
        return Ok(false);
    }
    for k in cadena {
        p.partes().1.boopsi.clases.insert(k, Clase { nativa: None, tarea: 0 });
    }
    Ok(true)
}

/// La clase de un objeto: la apuntada si lo hizo la caja (rootclass); si no, la de su cabecera (`OCLASS`: la palabra
/// de antes del objeto), si es una clase que se puede adoptar (los objetos de MUI 5, que hace su propia clase raíz).
pub(crate) fn clase_de_objeto<P: Puente>(p: &mut P, o: u32) -> R<Option<u32>> {
    if let Some(ob) = p.partes().1.boopsi.objetos.get(&o) {
        return Ok(Some(ob.clase));
    }
    if o < 4 || o & 1 != 0 || p.partes().0.tramo(o - 4, 8).is_err() {
        return Ok(None);
    }
    let c = l32(p, o - 4)?;
    Ok(adoptar_clase(p, c)?.then_some(c))
}

/// Manda el mensaje `msg` (en la memoria del 68k) a la clase `cl` para el objeto `obj`: su despachador, nativo o
/// 68k. Un fallo dentro se apunta y da 0; un programa que acaba dentro da `Cortado`.
pub(crate) fn enviar<P: Puente>(p: &mut P, cl: u32, obj: u32, msg: u32) -> R<u32> {
    {
        let (ex, gr) = p.partes();
        if gr.boopsi.niveles >= MAX_NIVELES {
            gr.avisar(ex, "(BOOPSI: demasiados métodos anidados)", Fallo::Sospechoso { dir: obj });
            return Ok(0);
        }
        gr.boopsi.niveles += 1;
    }
    let r = enviar_sin_tope(p, cl, obj, msg);
    let gr = p.partes().1;
    gr.boopsi.niveles = gr.boopsi.niveles.saturating_sub(1);
    match r {
        Err(Error::Fallo(f)) => {
            let (ex, gr) = p.partes();
            gr.avisar(ex, "(método BOOPSI)", f);
            Ok(0)
        }
        r => r,
    }
}

fn enviar_sin_tope<P: Puente>(p: &mut P, cl: u32, obj: u32, msg: u32) -> R<u32> {
    let (stub, conocida, base) = {
        let (_, gr) = p.partes();
        (gr.boopsi.stub, gr.boopsi.clases.contains_key(&cl), gr.intui.base)
    };
    if !conocida && !adoptar_clase(p, cl)? {
        return Err(Fallo::Sospechoso { dir: cl }.into());
    }
    let entrada = l32(p, cl + iclass::H_ENTRY)?;
    if entrada == stub {
        return despachar(p, cl, obj, msg);
    }
    if entrada == 0 || entrada & 1 != 0 {
        return Err(Fallo::Sospechoso { dir: cl }.into());
    }
    let mut regs = Registros::default();
    regs.a[0] = cl;
    regs.a[1] = msg;
    regs.a[2] = obj;
    regs.a[6] = base;
    match p.ejecutar_68k(entrada, &regs) {
        Ok(r) => Ok(r.d[0]),
        Err(Corte68k::Acabado) => Err(Error::Cortado),
        Err(Corte68k::NoSePudo) => Err(Fallo::Sospechoso { dir: entrada }.into()),
    }
}

/// Llama a un gancho (`struct Hook`) con `objeto` en A2 y `mensaje` en A1: su `h_Entry`, en 68k.
pub(crate) fn llamar_gancho<P: Puente>(p: &mut P, gancho: u32, objeto: u32, mensaje: u32) -> R<u32> {
    let entrada = l32(p, gancho + iclass::H_ENTRY)?;
    if entrada == 0 || entrada & 1 != 0 {
        return Err(Fallo::Sospechoso { dir: gancho }.into());
    }
    let mut regs = Registros::default();
    regs.a[0] = gancho;
    regs.a[1] = mensaje;
    regs.a[2] = objeto;
    match p.ejecutar_68k(entrada, &regs) {
        Ok(r) => Ok(r.d[0]),
        Err(Corte68k::Acabado) => Err(Error::Cortado),
        Err(Corte68k::NoSePudo) => Err(Fallo::Sospechoso { dir: entrada }.into()),
    }
}

/// `DoSuperMethodA`: el mensaje a la superclase de `cl`.
pub(crate) fn a_super<P: Puente>(p: &mut P, cl: u32, obj: u32, msg: u32) -> R<u32> {
    let s = l32(p, cl + iclass::SUPER)?;
    if s == 0 {
        return Ok(0);
    }
    enviar(p, s, obj, msg)
}

/// `DoMethodA` a un objeto, con el mensaje ya en memoria.
pub(crate) fn a_objeto<P: Puente>(p: &mut P, obj: u32, msg: u32) -> R<u32> {
    let cl = match clase_de(p, obj) {
        Ok(c) => c,
        Err(Error::Fallo(f)) => {
            let (ex, gr) = p.partes();
            gr.avisar(ex, "(método a algo que no es un objeto)", f);
            return Ok(0);
        }
        Err(e) => return Err(e),
    };
    enviar(p, cl, obj, msg)
}

/// `DoMethod` desde lo nativo: el mensaje con esas palabras.
pub(crate) fn metodo<P: Puente>(p: &mut P, obj: u32, palabras: &[u32]) -> R<u32> {
    if clase_de_objeto(p, obj)?.is_none() {
        return Ok(0);
    }
    con_mensaje(p, palabras, |p, m| a_objeto(p, obj, m))
}

/// El despachador de las clases nativas: la clase va en A0, el objeto en A2 y el mensaje en A1.
pub(crate) fn despachar<P: Puente>(p: &mut P, cl: u32, obj: u32, msg: u32) -> R<u32> {
    let Some((k, _)) = nativa_de(p, cl)? else { return Err(Fallo::Sospechoso { dir: cl }.into()) };
    let id = l32(p, msg)?;
    use Nativa::*;
    match k {
        Root => sistema::root(p, cl, obj, msg, id),
        Ic | Model => sistema::ic(p, k, cl, obj, msg, id),
        Image | FrameI | SysI | FillRect | IText => sistema::imagen(p, k, cl, obj, msg, id),
        Gadget => sistema::gadget(p, cl, obj, msg, id),
        PropG => sistema::prop(p, cl, obj, msg, id),
        StrG => sistema::cadena(p, cl, obj, msg, id),
        ButtonG | FrButton => sistema::boton(p, k, cl, obj, msg, id),
        GroupG => sistema::grupo(p, cl, obj, msg, id),
        _ => reaction::despachar(p, k, cl, obj, msg, id),
    }
}

// --- Las funciones de intuition ---

fn buscar_publica<P: Puente>(p: &mut P, nombre: &[u8]) -> R<u32> {
    let lista = p.partes().1.boopsi.publicas.clone();
    for cl in lista {
        let id = l32(p, cl + iclass::ID)?;
        if id != 0 && p.partes().0.cadena(id).ok().as_deref() == Some(nombre) {
            return Ok(cl);
        }
    }
    Ok(0)
}

/// `NewObjectA(class, classID, tagList)`: `OM_NEW` a la clase (el "objeto" es la clase de verdad).
pub(crate) fn new_object<P: Puente>(p: &mut P, cl: u32, id: u32, tags: u32) -> R<u32> {
    let clase = if cl != 0 {
        cl
    } else if id != 0 {
        let n = p.partes().0.cadena(id)?;
        buscar_publica(p, &n)?
    } else {
        0
    };
    if clase == 0 || (!p.partes().1.boopsi.clases.contains_key(&clase) && !adoptar_clase(p, clase)?) {
        return if cl != 0 { Err(Fallo::Sospechoso { dir: cl }.into()) } else { Ok(0) };
    }
    con_mensaje(p, &[om::NEW, tags, 0], |p, m| enviar(p, clase, clase, m))
}

/// `DisposeObject(object)`.
pub(crate) fn dispose_object<P: Puente>(p: &mut P, o: u32) -> R<()> {
    if o == 0 {
        return Ok(());
    }
    if clase_de_objeto(p, o)?.is_none() {
        return Err(Fallo::Sospechoso { dir: o }.into());
    }
    metodo(p, o, &[om::DISPOSE]).map(|_| ())
}

/// `SetAttrsA` (sin ventana) y `SetGadgetAttrsA` (con la de un gadget).
fn set_attrs<P: Puente>(p: &mut P, o: u32, w: u32, tags: u32) -> R<u32> {
    if o == 0 {
        return Ok(0);
    }
    if clase_de_objeto(p, o)?.is_none() {
        return Err(Fallo::Sospechoso { dir: o }.into());
    }
    let w = if p.partes().1.intui.ventanas.contains_key(&w) { w } else { 0 };
    con_ginfo(p, w, |p, gi| metodo(p, o, &[om::SET, tags, gi]))
}

/// `GetAttr(attrID, object, storage)`.
fn get_attr<P: Puente>(p: &mut P, id: u32, o: u32, storage: u32) -> R<u32> {
    if o == 0 || storage == 0 {
        return Ok(0);
    }
    if clase_de_objeto(p, o)?.is_none() {
        return Err(Fallo::Sospechoso { dir: o }.into());
    }
    p.partes().0.tramo(storage, 4)?;
    metodo(p, o, &[om::GET, id, storage])
}

/// `NextObject(objectPtrPtr)`: el objeto del nodo de `*objectPtrPtr` (su `o_Node`), y avanza.
fn next_object<P: Puente>(p: &mut P, pp: u32) -> R<u32> {
    let nodo = l32(p, pp)?;
    if nodo == 0 {
        return Ok(0);
    }
    let sig = l32(p, nodo)?;
    if sig == 0 {
        return Ok(0);
    }
    e32(p, pp, sig)?;
    Ok(nodo + CABECERA)
}

/// `MakeClass(classID, superClassID, superClassPtr, instanceSize, flags)`.
fn make_class<P: Puente>(p: &mut P, id: u32, sup_id: u32, sup: u32, tam: u32, _flags: u32) -> R<u32> {
    let s = if sup != 0 {
        sup
    } else if sup_id != 0 {
        let n = p.partes().0.cadena(sup_id)?;
        buscar_publica(p, &n)?
    } else {
        0
    };
    if s == 0 || (!p.partes().1.boopsi.clases.contains_key(&s) && !adoptar_clase(p, s)?) {
        return Ok(0);
    }
    let off = l16(p, s + iclass::INSTOFFSET)? as u32 + l16(p, s + iclass::INSTSIZE)? as u32;
    if off + tam > 0xFFFF || tam > 0xFFFF {
        return Err(Fallo::TamanoMalo { ancho: tam as i32, alto: 0 }.into());
    }
    let (ex, gr) = p.partes();
    let cl = ex.alloc_mem(iclass::TAMANO, memf::PUBLIC | memf::CLEAR)?;
    if cl == 0 {
        return Err(Fallo::SinMemoria.into());
    }
    ex.e32(cl + iclass::SUPER, s)?;
    ex.e32(cl + iclass::ID, id)?;
    ex.e16(cl + iclass::INSTOFFSET, off as u16)?;
    ex.e16(cl + iclass::INSTSIZE, tam as u16)?;
    let n = ex.l32(s + iclass::SUBCLASSCOUNT)?;
    ex.e32(s + iclass::SUBCLASSCOUNT, n + 1)?;
    // Una clase que hace una biblioteca de disco (MUI) es del sistema: la biblioteca sigue cuando acaba la tarea que
    // la abrió por primera vez, y sus clases con ella.
    let t = ex.dueno_de_lo_nuevo();
    gr.boopsi.clases.insert(cl, Clase { nativa: None, tarea: t });
    Ok(cl)
}

fn add_class<P: Puente>(p: &mut P, cl: u32) -> R<()> {
    adoptar_clase(p, cl)?;
    let (ex, gr) = p.partes();
    if !gr.boopsi.clases.contains_key(&cl) {
        return Err(Fallo::Sospechoso { dir: cl }.into());
    }
    if !gr.boopsi.publicas.contains(&cl) {
        gr.boopsi.publicas.push(cl);
    }
    let f = ex.l32(cl + iclass::FLAGS)?;
    ex.e32(cl + iclass::FLAGS, f | iclass::CLF_INLIST)?;
    Ok(())
}

fn remove_class<P: Puente>(p: &mut P, cl: u32) -> R<()> {
    let (ex, gr) = p.partes();
    if cl == 0 || !gr.boopsi.clases.contains_key(&cl) {
        return Ok(());
    }
    gr.boopsi.publicas.retain(|&c| c != cl);
    let f = ex.l32(cl + iclass::FLAGS)?;
    ex.e32(cl + iclass::FLAGS, f & !iclass::CLF_INLIST)?;
    Ok(())
}

/// `FreeClass(classPtr)`: FALSE si aún tiene objetos o subclases (o es del sistema).
fn free_class<P: Puente>(p: &mut P, cl: u32) -> R<u32> {
    let (ex, gr) = p.partes();
    let Some(c) = gr.boopsi.clases.get(&cl) else {
        if cl == 0 {
            return Ok(0);
        }
        return Err(Fallo::Sospechoso { dir: cl }.into());
    };
    if c.nativa.is_some() {
        return Ok(0);
    }
    if ex.l32(cl + iclass::OBJECTCOUNT)? != 0 || ex.l32(cl + iclass::SUBCLASSCOUNT)? != 0 {
        return Ok(0);
    }
    let sup = ex.l32(cl + iclass::SUPER)?;
    if gr.boopsi.clases.contains_key(&sup) {
        let n = ex.l32(sup + iclass::SUBCLASSCOUNT)?;
        ex.e32(sup + iclass::SUBCLASSCOUNT, n.saturating_sub(1))?;
    }
    gr.boopsi.clases.remove(&cl);
    gr.boopsi.publicas.retain(|&c| c != cl);
    ex.free_mem(cl, iclass::TAMANO)?;
    Ok(1)
}

/// `DoGadgetMethodA(gadget, window, requester, message)`: pone la `GadgetInfo` en el mensaje y lo manda.
fn do_gadget_method<P: Puente>(p: &mut P, g: u32, w: u32, msg: u32) -> R<u32> {
    if g == 0 || msg == 0 {
        return Ok(0);
    }
    if !p.partes().1.es_objeto(g) {
        return Err(Fallo::Sospechoso { dir: g }.into());
    }
    let w = if p.partes().1.intui.ventanas.contains_key(&w) { w } else { 0 };
    con_ginfo(p, w, |p, gi| {
        let id = l32(p, msg)?;
        // Los métodos de gadget (y OM_SET, OM_UPDATE, OM_NOTIFY) llevan la GadgetInfo en su segundo campo.
        if id <= gm::DOMAIN || matches!(id, om::SET | om::UPDATE | om::NOTIFY) {
            let campo = if matches!(id, om::SET | om::UPDATE | om::NOTIFY) { msg + 8 } else { msg + 4 };
            e32(p, campo, gi)?;
        }
        a_objeto(p, g, msg)
    })
}

// --- GadgetInfo, el rectángulo de un gadget y la cola ---

/// Una `GadgetInfo` de la ventana `w` (0: sin ventana, `gi` = 0) para lo que haga `f`.
pub(crate) fn con_ginfo<P: Puente, T>(p: &mut P, w: u32, f: impl FnOnce(&mut P, u32) -> R<T>) -> R<T> {
    if w == 0 {
        return f(p, 0);
    }
    let gi = {
        let (ex, gr) = p.partes();
        let Some(v) = gr.intui.ventanas.get(&w) else { return f(p, 0) };
        let (rp, capa) = (v.rp, v.capa);
        let (an, al) = (v.geometria.ancho, v.geometria.alto);
        // Su pantalla y la DrawInfo de ella (la de Workbench o una propia).
        let (pantalla, drinfo) = { let pa = gr.pantalla_de(w); (pa.dir, pa.drawinfo) };
        let gi = ex.reservar(ginfo::TAMANO + 2, memf::CLEAR)?;
        ex.e32(gi + ginfo::SCREEN, pantalla)?;
        ex.e32(gi + ginfo::WINDOW, w)?;
        ex.e32(gi + ginfo::RASTPORT, rp)?;
        ex.e32(gi + ginfo::LAYER, capa)?;
        ex.e16(gi + ginfo::DOMAIN + 4, an as u16)?;
        ex.e16(gi + ginfo::DOMAIN + 6, al as u16)?;
        let dp = ex.l8(w + crate::estructuras::window::DETAILPEN).unwrap_or(0);
        let bp = ex.l8(w + crate::estructuras::window::BLOCKPEN).unwrap_or(1);
        ex.e8(gi + ginfo::PENS, dp)?;
        ex.e8(gi + ginfo::PENS + 1, bp)?;
        ex.e32(gi + ginfo::DRINFO, drinfo)?;
        gi
    };
    let r = f(p, gi);
    let _ = p.partes().0.soltar(gi, ginfo::TAMANO + 2);
    r
}

/// La ventana de una `GadgetInfo` (0 si no hay o no es nuestra).
pub(crate) fn ventana_de<P: Puente>(p: &mut P, gi: u32) -> u32 {
    if gi == 0 {
        return 0;
    }
    let w = p.partes().0.l32(gi + ginfo::WINDOW).unwrap_or(0);
    if p.partes().1.intui.ventanas.contains_key(&w) { w } else { 0 }
}

/// El rectángulo de un gadget en su ventana, con los `GFLG_REL*` y la capa de dentro de una `GIMMEZEROZERO`.
pub(crate) fn caja_gadget<M: Memoria, C: Compositor>(ex: &Exec<M>, gr: &CajaGrafica<C>, w: u32, g: u32) -> Option<Rect> {
    use crate::gadtools::gadget as gd;
    let x = ex.l16(g.wrapping_add(gd::LEFTEDGE)).ok()? as i16 as i32;
    let y = ex.l16(g.wrapping_add(gd::TOPEDGE)).ok()? as i16 as i32;
    let mut an = ex.l16(g.wrapping_add(gd::WIDTH)).ok()? as i16 as i32;
    let mut al = ex.l16(g.wrapping_add(gd::HEIGHT)).ok()? as i16 as i32;
    let f = ex.l16(g.wrapping_add(gd::FLAGS)).ok()?;
    let v = gr.intui.ventanas.get(&w)?;
    let (van, val) = (v.geometria.ancho, v.geometria.alto);
    let x = if f & gflg::RELRIGHT != 0 { van + x - 1 } else { x };
    let y = if f & gflg::RELBOTTOM != 0 { val + y - 1 } else { y };
    if f & gflg::RELWIDTH != 0 {
        an += van;
    }
    if f & gflg::RELHEIGHT != 0 {
        al += val;
    }
    if an <= 0 || al <= 0 {
        return None;
    }
    let gzz = ex.l32(w.wrapping_add(crate::estructuras::window::FLAGS)).unwrap_or(0) & crate::estructuras::wflg::GIMMEZEROZERO != 0;
    let (dx, dy) = if gzz && ex.l16(g.wrapping_add(gd::GADGETTYPE)).unwrap_or(0) & gtyp::GZZGADGET == 0 {
        (
            ex.l8(w.wrapping_add(crate::estructuras::window::BORDERLEFT)).unwrap_or(0) as i32,
            ex.l8(w.wrapping_add(crate::estructuras::window::BORDERTOP)).unwrap_or(0) as i32,
        )
    } else {
        (0, 0)
    };
    Some(Rect::nuevo(x + dx, y + dy, x + dx + an - 1, y + dy + al - 1))
}

/// Atiende la cola: repintados, colocaciones y eventos de los gadgets BOOPSI.
pub(crate) fn atender_cola<P: Puente>(p: &mut P) -> R<()> {
    for _ in 0..MAX_COLA {
        let Some(t) = p.partes().1.boopsi.cola.pop_front() else { return Ok(()) };
        let r = match t {
            Pendiente::Pintar { w, g } => pintar_gadget(p, w, g, gm::GREDRAW_REDRAW),
            Pendiente::Colocar { w } => colocar(p, w),
            Pendiente::Entrada { w, e } => entrada(p, w, e),
        };
        match r {
            Err(Error::Cortado) => return Err(Error::Cortado),
            Err(Error::Fallo(f)) => {
                let (ex, gr) = p.partes();
                gr.avisar(ex, "(BOOPSI)", f);
            }
            Ok(()) => {}
        }
    }
    Ok(())
}

/// `GM_RENDER` a un gadget de la ventana.
pub(crate) fn pintar_gadget<P: Puente>(p: &mut P, w: u32, g: u32, modo: u32) -> R<()> {
    let rp = match p.partes().1.intui.ventanas.get(&w) {
        Some(v) => v.rp,
        None => return Ok(()),
    };
    if !p.partes().1.es_objeto(g) {
        return Ok(());
    }
    con_ginfo(p, w, |p, gi| metodo(p, g, &[gm::RENDER, gi, rp, modo])).map(|_| ())
}

/// `GM_LAYOUT` a los gadgets de la ventana con `GFLG_RELSPECIAL` o relativos, y los repinta.
fn colocar<P: Puente>(p: &mut P, w: u32) -> R<()> {
    let mut g = match p.partes().1.intui.ventanas.contains_key(&w) {
        true => l32(p, w + crate::estructuras::window::FIRSTGADGET)?,
        false => return Ok(()),
    };
    let mut lista = Vec::new();
    while g != 0 && lista.len() < 10_000 {
        lista.push(g);
        g = l32(p, g)?;
    }
    for g in lista {
        let (ex, gr) = p.partes();
        if !gr.es_gadget_boopsi(ex, g) {
            continue;
        }
        let f = l16(p, g + crate::gadtools::gadget::FLAGS)?;
        if f & (gflg::RELSPECIAL | gflg::RELRIGHT | gflg::RELBOTTOM | gflg::RELWIDTH | gflg::RELHEIGHT) != 0 {
            con_ginfo(p, w, |p, gi| metodo(p, g, &[gm::LAYOUT, gi, 0]))?;
        }
        pintar_gadget(p, w, g, gm::GREDRAW_REDRAW)?;
    }
    Ok(())
}

/// Un `InputEvent` en la memoria del 68k.
fn con_evento<P: Puente, T>(p: &mut P, clase: u8, codigo: u16, calif: u16, f: impl FnOnce(&mut P, u32) -> R<T>) -> R<T> {
    let ie = p.partes().0.reservar(ievent::TAMANO + 2, memf::CLEAR)?;
    {
        let (ex, gr) = p.partes();
        ex.e8(ie + ievent::CLASS, clase)?;
        ex.e16(ie + ievent::CODE, codigo)?;
        ex.e16(ie + ievent::QUALIFIER, calif | gr.intui.calif.bits)?;
        let (s, us) = gr.intui.hora;
        ex.e32(ie + ievent::TIMESTAMP, s)?;
        ex.e32(ie + ievent::TIMESTAMP + 4, us)?;
    }
    let r = f(p, ie);
    let _ = p.partes().0.soltar(ie, ievent::TAMANO + 2);
    r
}

/// El gadget BOOPSI (de la lista de la ventana, sin los desactivados) que hay en (x, y).
fn gadget_en<P: Puente>(p: &mut P, w: u32, x: i32, y: i32) -> R<Option<(u32, Rect)>> {
    let mut g = l32(p, w + crate::estructuras::window::FIRSTGADGET)?;
    let mut k = 0;
    while g != 0 && k < 10_000 {
        k += 1;
        let (ex, gr) = p.partes();
        let f = ex.l16(g + crate::gadtools::gadget::FLAGS).unwrap_or(0);
        if gr.es_gadget_boopsi(ex, g)
            && f & gflg::DISABLED == 0
            && let Some(r) = caja_gadget(ex, gr, w, g).filter(|r| r.contiene(x, y))
        {
            return Ok(Some((g, r)));
        }
        g = ex.l32(g).unwrap_or(0);
    }
    Ok(None)
}

/// Un evento para los gadgets BOOPSI de la ventana: el activo lo recibe con `GM_HANDLEINPUT`; un clic sin activo,
/// `GM_HITTEST` y `GM_GOACTIVE` al de debajo.
fn entrada<P: Puente>(p: &mut P, w: u32, e: Entrada) -> R<()> {
    if !p.partes().1.intui.ventanas.contains_key(&w) {
        return Ok(());
    }
    let activo = p.partes().1.boopsi.activo.filter(|a| a.w == w);
    let (clase, codigo, calif, x, y) = match e {
        Entrada::Pulsar { x, y } => (ievent::CLASS_RAWMOUSE, ievent::LBUTTON, 0x4000, x, y),
        Entrada::Soltar { x, y } => (ievent::CLASS_RAWMOUSE, ievent::LBUTTON | ievent::UP_PREFIX, 0, x, y),
        Entrada::Mover { x, y } => (ievent::CLASS_RAWMOUSE, ievent::NOBUTTON, 0x4000, x, y),
        Entrada::Tecla { crudo, calif, .. } => {
            let (ex, _) = p.partes();
            let mx = ex.l16(w + crate::estructuras::window::MOUSEX)? as i16 as i32;
            let my = ex.l16(w + crate::estructuras::window::MOUSEY)? as i16 as i32;
            (ievent::CLASS_RAWKEY, crudo as u16, calif, mx, my)
        }
    };
    p.partes().1.boopsi.caracter = match e {
        Entrada::Tecla { ch, .. } => ch,
        _ => None,
    };
    match activo {
        Some(a) => {
            let Some(r) = ({
                let (ex, gr) = p.partes();
                caja_gadget(ex, gr, w, a.g)
            }) else {
                p.partes().1.boopsi.activo = None;
                return Ok(());
            };
            let res = con_ginfo(p, w, |p, gi| {
                con_evento(p, clase, codigo, calif, |p, ie| {
                    con_mensaje(p, &[0, 0], |p, term| {
                        p.partes().1.boopsi.hoja = 0;
                        let rel = ((x - r.x0) as u16 as u32) << 16 | (y - r.y0) as u16 as u32;
                        let v = metodo(p, a.g, &[gm::HANDLEINPUT, gi, ie, term, rel, 0])?;
                        Ok((v, l32(p, term)?))
                    })
                })
            })?;
            if res.0 != gm::GMR_MEACTIVE {
                acabar(p, w, a.g, res.0, res.1)?;
                // GMR_REUSE: el clic que lo desactivó vale para lo que haya debajo.
                if res.0 & gm::GMR_REUSE != 0
                    && let Entrada::Pulsar { x, y } = e
                {
                    return pulsar(p, w, x, y);
                }
            }
            Ok(())
        }
        None => match e {
            Entrada::Pulsar { x, y } => pulsar(p, w, x, y),
            _ => Ok(()),
        },
    }
}

/// Un clic sin gadget activo: `GM_HITTEST` y `GM_GOACTIVE` al gadget BOOPSI de debajo.
fn pulsar<P: Puente>(p: &mut P, w: u32, x: i32, y: i32) -> R<()> {
    let Some((g, r)) = gadget_en(p, w, x, y)? else {
        // No era un gadget BOOPSI: lo de GadTools o intuition.
        let (ex, gr) = p.partes();
        if let Some(Err(f)) = gr.gadget_pulsar_no_boopsi(ex, w, x, y) {
            return Err(f.into());
        }
        return Ok(());
    };
    let rel = ((x - r.x0) as u16 as u32) << 16 | (y - r.y0) as u16 as u32;
    let golpe = con_ginfo(p, w, |p, gi| metodo(p, g, &[gm::HITTEST, gi, rel]))?;
    if golpe != gm::GMR_GADGETHIT {
        return Ok(());
    }
    let res = con_ginfo(p, w, |p, gi| {
        con_evento(p, ievent::CLASS_RAWMOUSE, ievent::LBUTTON, 0, |p, ie| {
            con_mensaje(p, &[0, 0], |p, term| {
                p.partes().1.boopsi.hoja = 0;
                let v = metodo(p, g, &[gm::GOACTIVE, gi, ie, term, rel, 0])?;
                Ok((v, l32(p, term)?))
            })
        })
    })?;
    let act = l16(p, g + crate::gadtools::gadget::ACTIVATION)?;
    if act & gact::IMMEDIATE != 0 {
        let (ex, gr) = p.partes();
        gr.enviar(ex, w, crate::estructuras::idcmp::GADGETDOWN, 0, g)?;
    }
    if res.0 == gm::GMR_MEACTIVE {
        p.partes().1.boopsi.activo = Some(Activo { w, g });
        let a = l16(p, g + crate::gadtools::gadget::ACTIVATION)?;
        e16(p, g + crate::gadtools::gadget::ACTIVATION, a | gact::ACTIVEGADGET)?;
    } else {
        acabar(p, w, g, res.0, res.1)?;
    }
    Ok(())
}

/// El gadget dejó de estar activo (lo que devolvió no era `GMR_MEACTIVE`): `GM_GOINACTIVE` y, con `GMR_VERIFY`,
/// `IDCMP_GADGETUP` (con la hoja que terminó, si era un hijo de un layout).
fn acabar<P: Puente>(p: &mut P, w: u32, g: u32, res: u32, term: u32) -> R<()> {
    let hoja = p.partes().1.boopsi.hoja;
    if p.partes().1.boopsi.activo.is_some_and(|a| a.g == g) {
        p.partes().1.boopsi.activo = None;
        con_ginfo(p, w, |p, gi| metodo(p, g, &[gm::GOINACTIVE, gi, 0]))?;
        if p.partes().1.es_objeto(g) {
            let a = l16(p, g + crate::gadtools::gadget::ACTIVATION)?;
            e16(p, g + crate::gadtools::gadget::ACTIVATION, a & !gact::ACTIVEGADGET)?;
        }
    }
    if res & gm::GMR_VERIFY != 0 && p.partes().1.es_objeto(g) {
        let destino = if hoja != 0 && p.partes().1.es_objeto(hoja) { hoja } else { g };
        let act = l16(p, destino + crate::gadtools::gadget::ACTIVATION)?;
        if act & gact::RELVERIFY != 0 {
            let (ex, gr) = p.partes();
            gr.enviar(ex, w, crate::estructuras::idcmp::GADGETUP, term as u16, destino)?;
        }
    }
    p.partes().1.boopsi.hoja = 0;
    Ok(())
}
