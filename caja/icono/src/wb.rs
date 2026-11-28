//! workbench.library sin Workbench con iconos.
//!
//! La caja aún no tiene un Workbench que enseñe iconos y ventanas de cajones. Lo que piden los programas para estar
//! en él (`AddAppWindowA`, `AddAppIconA`, `AddAppMenuItemA`, `AddAppWindowDropZoneA`) se da como si estuviera
//! abierto: un objeto válido (un bloque privado en la memoria del 68k, que es lo que dice el include: `struct
//! AppWindow { void *aw_PRIVATE; }`), apuntado en una tabla con su puerto, su id y sus datos, para que el día que
//! haya Workbench se les entreguen sus `AppMessage` (`entregar`; las respuestas vuelven a un puerto propio y se
//! sueltan con `recoger`). Con `poner_abierto(false)`, devuelven NULL, como dice el autodoc sin Workbench.
//!
//! Lo que necesita un Workbench de verdad (abrir o cerrar un cajón, la selección, hacer visible un icono, la
//! ventana de información) devuelve lo que dice su autodoc cuando no encuentra lo que se pide: `FALSE` con
//! `ERROR_OBJECT_NOT_FOUND` en `IoErr()`. `OpenWorkbenchObjectA` de un programa o de un proyecto sí hace algo: busca
//! la herramienta (la del icono del proyecto) y pide a la caja que la lance (`Entorno::lanzar`).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_exec::estructuras::{memf, message, msgport, node, nt};
use caja_exec::{Accion, Exec, Fallo, Memoria, Registros};

use crate::errores::*;
use crate::info::{WBPROJECT, WBTOOL};
use crate::mem::{cadena, e8, e16, e32, escribir, etiquetas, poner, valor};
use crate::{Clase, Entorno, Iconos};

/// Etiquetas de `workbench/workbench.h` (`WBA_Dummy` = `TAG_USER + 0xA000`).
pub mod tags {
    const D: u32 = 0x8000_A000;
    pub const WBAPPICONA_SUPPORTSOPEN: u32 = D + 1;
    pub const WBAPPICONA_SUPPORTSEMPTYTRASH: u32 = D + 11;
    pub const WBAPPICONA_PROPAGATEPOSITION: u32 = D + 12;
    pub const WBAPPICONA_RENDERHOOK: u32 = D + 13;
    pub const WBAPPICONA_NOTIFYSELECTSTATE: u32 = D + 14;
    pub const WBAPPMENUA_COMMANDKEYSTRING: u32 = D + 15;
    pub const WBAPPMENUA_GETKEY: u32 = D + 65;
    pub const WBAPPMENUA_USEKEY: u32 = D + 66;
    pub const WBAPPMENUA_GETTITLEKEY: u32 = D + 77;
    pub const WBOPENA_ARGLOCK: u32 = D + 16;
    pub const WBOPENA_ARGNAME: u32 = D + 17;
    pub const WBCTRLA_ISOPEN: u32 = D + 18;
    pub const WBCTRLA_DUPLICATESEARCHPATH: u32 = D + 19;
    pub const WBCTRLA_FREESEARCHPATH: u32 = D + 20;
    pub const WBCTRLA_GETDEFAULTSTACKSIZE: u32 = D + 21;
    pub const WBCTRLA_SETDEFAULTSTACKSIZE: u32 = D + 22;
    pub const WBCTRLA_REDRAWAPPICON: u32 = D + 23;
    pub const WBCTRLA_GETPROGRAMLIST: u32 = D + 24;
    pub const WBCTRLA_FREEPROGRAMLIST: u32 = D + 25;
    pub const WBCTRLA_GETSELECTEDICONLIST: u32 = D + 36;
    pub const WBCTRLA_FREESELECTEDICONLIST: u32 = D + 37;
    pub const WBCTRLA_GETOPENDRAWERLIST: u32 = D + 38;
    pub const WBCTRLA_FREEOPENDRAWERLIST: u32 = D + 39;
    pub const WBCTRLA_GETHIDDENDEVICELIST: u32 = D + 42;
    pub const WBCTRLA_FREEHIDDENDEVICELIST: u32 = D + 43;
    pub const WBCTRLA_ADDHIDDENDEVICENAME: u32 = D + 44;
    pub const WBCTRLA_REMOVEHIDDENDEVICENAME: u32 = D + 45;
    pub const WBCTRLA_GETTYPERESTARTTIME: u32 = D + 47;
    pub const WBCTRLA_SETTYPERESTARTTIME: u32 = D + 48;
    pub const WBCTRLA_GETCOPYHOOK: u32 = D + 69;
    pub const WBCTRLA_SETCOPYHOOK: u32 = D + 70;
    pub const WBCTRLA_GETDELETEHOOK: u32 = D + 71;
    pub const WBCTRLA_SETDELETEHOOK: u32 = D + 72;
    pub const WBCTRLA_GETTEXTINPUTHOOK: u32 = D + 73;
    pub const WBCTRLA_SETTEXTINPUTHOOK: u32 = D + 74;
    pub const WBCTRLA_ADDSETUPCLEANUPHOOK: u32 = D + 78;
    pub const WBCTRLA_REMSETUPCLEANUPHOOK: u32 = D + 79;
    pub const WBCTRLA_SETGLOBALFLAGS: u32 = D + 130;
    pub const WBCTRLA_GETGLOBALFLAGS: u32 = D + 131;
    pub const WBCTRLA_GETDISKINFOHOOK: u32 = D + 159;
    pub const WBCTRLA_SETDISKINFOHOOK: u32 = D + 160;
    pub const WBDZA_LEFT: u32 = D + 26;
    pub const WBDZA_RELRIGHT: u32 = D + 27;
    pub const WBDZA_TOP: u32 = D + 28;
    pub const WBDZA_RELBOTTOM: u32 = D + 29;
    pub const WBDZA_WIDTH: u32 = D + 30;
    pub const WBDZA_RELWIDTH: u32 = D + 31;
    pub const WBDZA_HEIGHT: u32 = D + 32;
    pub const WBDZA_RELHEIGHT: u32 = D + 33;
    pub const WBDZA_BOX: u32 = D + 34;
    pub const WBDZA_HOOK: u32 = D + 35;
}
use tags::*;

/// `struct AppMessage` (86 bytes) y `struct WBArg` (8).
pub mod appmessage {
    pub const AM_TYPE: u32 = 20;
    pub const AM_USERDATA: u32 = 22;
    pub const AM_ID: u32 = 26;
    pub const AM_NUMARGS: u32 = 30;
    pub const AM_ARGLIST: u32 = 34;
    pub const AM_VERSION: u32 = 38;
    pub const AM_CLASS: u32 = 40;
    pub const AM_MOUSEX: u32 = 42;
    pub const AM_MOUSEY: u32 = 44;
    pub const AM_SECONDS: u32 = 46;
    pub const AM_MICROS: u32 = 50;
    pub const AM_RESERVED: u32 = 54;
    pub const TAMANO: u32 = 86;
    pub const WBARG: u32 = 8;
    pub const AM_VERSION_ACTUAL: u16 = 1;
    pub const AMTYPE_APPWINDOW: u16 = 7;
    pub const AMTYPE_APPICON: u16 = 8;
    pub const AMTYPE_APPMENUITEM: u16 = 9;
    pub const AMTYPE_APPWINDOWZONE: u16 = 10;
}

/// Lo que mide el bloque privado de cada objeto App.
const PRIVADO: u32 = 16;
/// Elementos del menú Herramientas como mucho (autodoc de `AddAppMenuItemA`).
const MAX_MENU: usize = 62;
/// Objetos App como mucho (todos los de todas las tareas).
const MAX_OBJETOS: usize = 4096;
/// `WBO_NONE`.
const WBO_NONE: u32 = 0;
/// La pila por defecto de lo que lanza el Workbench.
const PILA_DE_SERIE: u32 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoApp {
    Ventana,
    Icono,
    Menu,
    /// Una zona de una ventana App (`ventana` es la `AppWindow`).
    Zona,
}

/// Un objeto App dado a un programa.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjetoApp {
    /// Su dirección (el bloque privado): lo que recibe el programa.
    pub dir: u32,
    pub tipo: TipoApp,
    pub id: u32,
    pub datos: u32,
    /// La `Window` de una ventana App; la `AppWindow` de una zona.
    pub ventana: u32,
    pub puerto: u32,
    /// El nombre de un icono o de un menú.
    pub texto: Vec<u8>,
    /// El `DiskObject` de un icono App.
    pub icono: u32,
    pub tarea: u32,
    /// Una zona: izquierda, arriba, ancho, alto, y si cada uno es relativo (a la derecha, abajo, al ancho, al alto).
    pub caja: [i16; 4],
    pub relativa: [bool; 4],
    pub gancho: u32,
    /// Una tecla de menú (`WBAPPMENUA_CommandKeyString`), o 0; y la clave del menú padre (`WBAPPMENUA_UseKey`).
    pub tecla: u8,
    pub padre: u32,
}

/// El estado de workbench.library.
#[derive(Debug)]
pub struct Workbench {
    /// Si hace como que hay un Workbench abierto (los objetos App se dan). De serie, sí.
    pub abierto: bool,
    objetos: Vec<ObjetoApp>,
    pila: u32,
    reinicio: u32,
    banderas: u32,
    ocultos: Vec<Vec<u8>>,
    /// Ganchos de `WorkbenchControlA` (copiar, borrar, texto, disco); nadie los llama aún.
    ganchos: BTreeMap<u32, u32>,
    limpieza: Vec<u32>,
    /// Listas dadas por `WBCTRLA_Get...List`: lista → (tarea, bloques que la forman).
    listas: BTreeMap<u32, (u32, Vec<(u32, u32)>)>,
    /// Claves de menú dadas (`WBAPPMENUA_GetKey`).
    siguiente_clave: u32,
    /// El puerto propio (sin señal) a donde vuelven los `AppMessage`, y los que están fuera: mensaje → largo.
    puerto: u32,
    fuera: BTreeMap<u32, u32>,
}

impl Default for Workbench {
    fn default() -> Self {
        Workbench {
            abierto: true,
            objetos: Vec::new(),
            pila: PILA_DE_SERIE,
            reinicio: 2,
            banderas: 0,
            ocultos: Vec::new(),
            ganchos: BTreeMap::new(),
            limpieza: Vec::new(),
            listas: BTreeMap::new(),
            siguiente_clave: 1,
            puerto: 0,
            fuera: BTreeMap::new(),
        }
    }
}

impl Workbench {
    pub fn objetos(&self) -> &[ObjetoApp] {
        &self.objetos
    }
    pub(crate) fn tarea_muerta(&mut self, t: u32) {
        let ventanas: Vec<u32> =
            self.objetos.iter().filter(|o| o.tarea == t && o.tipo == TipoApp::Ventana).map(|o| o.dir).collect();
        self.objetos.retain(|o| o.tarea != t && !(o.tipo == TipoApp::Zona && ventanas.contains(&o.ventana)));
        self.listas.retain(|_, l| l.0 != t);
    }
    /// Los mensajes entregados y aún no devueltos.
    pub fn mensajes_fuera(&self) -> usize {
        self.fuera.len()
    }
}

/// Un argumento de un `AppMessage`: el cerrojo (BPTR, de quien lo entrega; el programa no lo suelta) y el nombre.
pub struct Argumento<'a> {
    pub cerrojo: u32,
    pub nombre: &'a [u8],
}

impl Iconos {
    /// Si hace como que hay un Workbench abierto: con `false`, los `AddApp...` devuelven NULL.
    pub fn poner_abierto(&mut self, si: bool) {
        self.wb.abierto = si;
    }

    pub(crate) fn llamar_wb<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        nombre: &'static str,
        r: &mut Registros,
        en: &mut E,
    ) -> Result<(), Fallo> {
        let (d, a) = (r.d, r.a);
        r.d[0] = match nombre {
            "UpdateWorkbench" => 0,
            "AddAppWindowA" => {
                if a[0] == 0 || a[1] == 0 {
                    return Err(Fallo::PunteroNulo);
                }
                let o = ObjetoApp::nuevo(TipoApp::Ventana, d[0], d[1], a[1]);
                self.dar_app(ex, ObjetoApp { ventana: a[0], ..o })?
            }
            "AddAppIconA" => {
                if a[1] == 0 || a[3] == 0 {
                    return Err(Fallo::PunteroNulo);
                }
                crate::mem::comprobar(a[3], crate::diskobj::dobj::TAMANO, false)?;
                let texto = if a[0] == 0 { Vec::new() } else { cadena(ex, a[0])? };
                let et = etiquetas(ex, a[4])?;
                let o = ObjetoApp::nuevo(TipoApp::Icono, d[0], d[1], a[1]);
                self.dar_app(ex, ObjetoApp { texto, icono: a[3], gancho: valor(&et, WBAPPICONA_RENDERHOOK, 0), ..o })?
            }
            "AddAppMenuItemA" => {
                if a[0] == 0 || a[1] == 0 {
                    return Err(Fallo::PunteroNulo);
                }
                let texto = cadena(ex, a[0])?;
                let et = etiquetas(ex, a[2])?;
                let padre = valor(&et, WBAPPMENUA_USEKEY, 0);
                let menus = self.wb.objetos.iter().filter(|o| o.tipo == TipoApp::Menu && o.padre == padre).count();
                if menus >= MAX_MENU {
                    0
                } else {
                    let tecla = match valor(&et, WBAPPMENUA_COMMANDKEYSTRING, 0) {
                        0 => 0,
                        p => cadena(ex, p)?.first().copied().unwrap_or(0),
                    };
                    let o = ObjetoApp::nuevo(TipoApp::Menu, d[0], d[1], a[1]);
                    let dir = self.dar_app(ex, ObjetoApp { texto, tecla, padre, ..o })?;
                    if dir != 0 {
                        for t in [WBAPPMENUA_GETKEY, WBAPPMENUA_GETTITLEKEY] {
                            if let Some(p) = et.iter().find(|e| e.tag == t).map(|e| e.dato) {
                                let k = self.wb.siguiente_clave;
                                self.wb.siguiente_clave = k.wrapping_add(1).max(1);
                                poner(ex, p, k)?;
                            }
                        }
                    }
                    dir
                }
            }
            "RemoveAppWindow" => self.quitar_app(ex, a[0], TipoApp::Ventana)? as u32,
            "RemoveAppIcon" => self.quitar_app(ex, a[0], TipoApp::Icono)? as u32,
            "RemoveAppMenuItem" => self.quitar_app(ex, a[0], TipoApp::Menu)? as u32,
            "AddAppWindowDropZoneA" => self.add_zona(ex, en, a[0], d[0], d[1], a[1])?,
            "RemoveAppWindowDropZone" => {
                if a[0] == 0 || a[1] == 0 {
                    0
                } else if self.wb.objetos.iter().any(|o| o.dir == a[1] && o.tipo == TipoApp::Zona && o.ventana == a[0])
                {
                    self.quitar_app(ex, a[1], TipoApp::Zona)? as u32
                } else {
                    en.poner_io_err(ex, ERROR_OBJECT_NOT_FOUND);
                    0
                }
            }
            "WBInfo" => 0,
            "OpenWorkbenchObjectA" => self.abrir_objeto(ex, en, a[0], a[1])?,
            "CloseWorkbenchObjectA" | "MakeWorkbenchObjectVisibleA" => {
                // Ningún cajón está abierto: no se encuentra.
                cadena(ex, a[0])?;
                en.poner_io_err(ex, ERROR_OBJECT_NOT_FOUND);
                0
            }
            "ChangeWorkbenchSelectionA" => {
                if a[0] == 0 {
                    // El fondo, sin iconos: el gancho no tiene a quién llamar.
                    1
                } else {
                    cadena(ex, a[0])?;
                    en.poner_io_err(ex, ERROR_OBJECT_NOT_FOUND);
                    0
                }
            }
            "WorkbenchControlA" => self.control(ex, en, a[0], a[1])?,
            "WhichWorkbenchObjectA" => WBO_NONE,
            _ => return Err(Fallo::NoImplementada { n: 0 }),
        };
        Ok(())
    }

    fn dar_app<M: Memoria>(&mut self, ex: &mut Exec<M>, o: ObjetoApp) -> Result<u32, Fallo> {
        if !self.wb.abierto || self.wb.objetos.len() >= MAX_OBJETOS {
            return Ok(0);
        }
        let dir = ex.alloc_mem(PRIVADO, memf::PUBLIC | memf::CLEAR)?;
        if dir == 0 {
            return Ok(0);
        }
        let tarea = ex.dueno_de_lo_nuevo();
        self.wb.objetos.push(ObjetoApp { dir, tarea, ..o });
        Ok(dir)
    }

    fn quitar_app<M: Memoria>(&mut self, ex: &mut Exec<M>, dir: u32, tipo: TipoApp) -> Result<bool, Fallo> {
        let Some(k) = self.wb.objetos.iter().position(|o| o.dir == dir && o.tipo == tipo) else {
            if dir == 0 {
                return Ok(false);
            }
            return Err(Fallo::NoReservado { dir });
        };
        self.wb.objetos.remove(k);
        // Las zonas de una ventana se van con ella.
        if tipo == TipoApp::Ventana {
            let zonas: Vec<u32> =
                self.wb.objetos.iter().filter(|o| o.tipo == TipoApp::Zona && o.ventana == dir).map(|o| o.dir).collect();
            for z in zonas {
                self.wb.objetos.retain(|o| o.dir != z);
                ex.free_mem(z, PRIVADO)?;
            }
        }
        ex.free_mem(dir, PRIVADO)?;
        Ok(true)
    }

    fn add_zona<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        aw: u32,
        id: u32,
        datos: u32,
        lista: u32,
    ) -> Result<u32, Fallo> {
        let Some(ventana) = self.wb.objetos.iter().find(|o| o.dir == aw && o.tipo == TipoApp::Ventana).cloned() else {
            en.poner_io_err(ex, ERROR_OBJECT_NOT_FOUND);
            return if aw == 0 { Ok(0) } else { Err(Fallo::NoReservado { dir: aw }) };
        };
        let et = etiquetas(ex, lista)?;
        let mut caja = [0i16; 4];
        let mut relativa = [false; 4];
        let mut hay = [false; 4];
        for e in &et {
            let v = e.dato as i16;
            let (k, rel) = match e.tag {
                WBDZA_LEFT => (0, false),
                WBDZA_RELRIGHT => (0, true),
                WBDZA_TOP => (1, false),
                WBDZA_RELBOTTOM => (1, true),
                WBDZA_WIDTH => (2, false),
                WBDZA_RELWIDTH => (2, true),
                WBDZA_HEIGHT => (3, false),
                WBDZA_RELHEIGHT => (3, true),
                WBDZA_BOX => {
                    for (j, c) in caja.iter_mut().enumerate() {
                        *c = crate::mem::l16(ex, e.dato.checked_add(2 * j as u32).ok_or(Fallo::PunteroNulo)?)? as i16;
                    }
                    (relativa, hay) = ([false; 4], [true; 4]);
                    continue;
                }
                _ => continue,
            };
            (caja[k], relativa[k], hay[k]) = (v, rel, true);
        }
        if !hay.iter().all(|&h| h) {
            // Sin sitio o sin tamaño no se crea (autodoc).
            en.poner_io_err(ex, ERROR_REQUIRED_ARG_MISSING);
            return Ok(0);
        }
        let o = ObjetoApp::nuevo(TipoApp::Zona, id, datos, ventana.puerto);
        let dir =
            self.dar_app(ex, ObjetoApp { ventana: aw, caja, relativa, gancho: valor(&et, WBDZA_HOOK, 0), ..o })?;
        if dir != 0 {
            // Una zona es de la tarea de su ventana.
            if let Some(z) = self.wb.objetos.iter_mut().find(|o| o.dir == dir) {
                z.tarea = ventana.tarea;
            }
        } else {
            en.poner_io_err(ex, ERROR_NO_FREE_STORE);
        }
        Ok(dir)
    }

    /// `OpenWorkbenchObjectA`: un programa o un proyecto se lanzan (por la caja); un cajón no se puede abrir.
    fn abrir_objeto<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        nombre: u32,
        lista: u32,
    ) -> Result<u32, Fallo> {
        let n = cadena(ex, nombre)?;
        let et = etiquetas(ex, lista)?;
        let r = (|| -> Result<(), u32> {
            let clase = if n.last() == Some(&b':') { Clase::Disco } else { en.clase(ex, &n)? };
            let icono = self.cargar(ex, en, &n, false).ok();
            let mut args: Vec<(u32, Vec<u8>)> = Vec::new();
            let mut pila = self.wb.pila;
            match clase {
                Clase::Disco | Clase::Cajon => {
                    self.avisar(ex, "OpenWorkbenchObjectA", Fallo::NoImplementada { n: 0 });
                    return Err(ERROR_NOT_IMPLEMENTED);
                }
                Clase::Programa => {
                    if let Some(i) = icono.as_ref().filter(|i| i.tipo == WBTOOL && i.pila > 0) {
                        pila = i.pila as u32;
                    }
                    args.push((0, n.clone()));
                }
                Clase::Proyecto => {
                    let i = icono.ok_or(ERROR_OBJECT_WRONG_TYPE)?;
                    let herr = i
                        .herramienta
                        .clone()
                        .filter(|h| !h.is_empty() && i.tipo == WBPROJECT)
                        .ok_or(ERROR_OBJECT_WRONG_TYPE)?;
                    if i.pila > 0 {
                        pila = i.pila as u32;
                    }
                    args.push((0, herr));
                    args.push((0, n.clone()));
                }
            }
            // Los argumentos de las etiquetas (no se usan al abrir un cajón).
            let mut cerrojo = 0;
            for e in &et {
                match e.tag {
                    WBOPENA_ARGLOCK => cerrojo = e.dato,
                    WBOPENA_ARGNAME => args.push((cerrojo, cadena(ex, e.dato).map_err(|_| ERROR_BAD_NUMBER)?)),
                    _ => {}
                }
            }
            en.lanzar(ex, &args, pila)
        })();
        Ok(match r {
            Ok(()) => 1,
            Err(e) => {
                en.poner_io_err(ex, e);
                0
            }
        })
    }

    /// Una lista vacía (o con estos nombres en sus nodos) para el programa; se suelta con su `WBCTRLA_Free...List`.
    fn dar_lista<M: Memoria>(&mut self, ex: &mut Exec<M>, nombres: &[Vec<u8>]) -> Result<u32, Fallo> {
        let l = ex.alloc_mem(16, memf::PUBLIC | memf::CLEAR)?;
        if l == 0 {
            return Ok(0);
        }
        ex.new_list(l, 0)?;
        let mut bloques = alloc::vec![(l, 16)];
        for n in nombres {
            let largo = node::TAMANO + n.len() as u32 + 1;
            let nd = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
            if nd == 0 {
                break;
            }
            escribir(ex, nd + node::TAMANO, n)?;
            e32(ex, nd + node::LN_NAME, nd + node::TAMANO)?;
            ex.add_tail(l, nd)?;
            bloques.push((nd, largo));
        }
        let t = ex.tarea_actual();
        self.wb.listas.insert(l, (t, bloques));
        Ok(l)
    }

    fn soltar_lista<M: Memoria>(&mut self, ex: &mut Exec<M>, l: u32) -> Result<(), Fallo> {
        if l == 0 {
            return Ok(());
        }
        let (_, bloques) = self.wb.listas.remove(&l).ok_or(Fallo::NoReservado { dir: l })?;
        for (d, largo) in bloques {
            ex.free_mem(d, largo)?;
        }
        Ok(())
    }

    fn control<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        en: &mut E,
        nombre: u32,
        lista: u32,
    ) -> Result<u32, Fallo> {
        let et = etiquetas(ex, lista)?;
        if nombre != 0 {
            cadena(ex, nombre)?;
        }
        for e in &et {
            let p = e.dato;
            match e.tag {
                // Ningún cajón está abierto.
                WBCTRLA_ISOPEN => poner(ex, p, 0)?,
                // Sin camino de búsqueda propio: la lista vacía (BPTR 0).
                WBCTRLA_DUPLICATESEARCHPATH => poner(ex, p, 0)?,
                WBCTRLA_FREESEARCHPATH | WBCTRLA_REDRAWAPPICON => {}
                WBCTRLA_GETDEFAULTSTACKSIZE => poner(ex, p, self.wb.pila)?,
                WBCTRLA_SETDEFAULTSTACKSIZE => self.wb.pila = p.clamp(4096, 16 << 20),
                WBCTRLA_GETTYPERESTARTTIME => poner(ex, p, self.wb.reinicio)?,
                WBCTRLA_SETTYPERESTARTTIME => self.wb.reinicio = p.min(3600),
                WBCTRLA_GETGLOBALFLAGS => poner(ex, p, self.wb.banderas)?,
                WBCTRLA_SETGLOBALFLAGS => self.wb.banderas = p,
                WBCTRLA_GETPROGRAMLIST | WBCTRLA_GETSELECTEDICONLIST | WBCTRLA_GETOPENDRAWERLIST => {
                    let l = self.dar_lista(ex, &[])?;
                    poner(ex, p, l)?
                }
                WBCTRLA_GETHIDDENDEVICELIST => {
                    let nombres = self.wb.ocultos.clone();
                    let l = self.dar_lista(ex, &nombres)?;
                    poner(ex, p, l)?
                }
                WBCTRLA_FREEPROGRAMLIST
                | WBCTRLA_FREESELECTEDICONLIST
                | WBCTRLA_FREEOPENDRAWERLIST
                | WBCTRLA_FREEHIDDENDEVICELIST => self.soltar_lista(ex, p)?,
                WBCTRLA_ADDHIDDENDEVICENAME => {
                    let n = cadena(ex, p)?;
                    if !self.wb.ocultos.iter().any(|o| crate::mem::iguales_sin_caja(o, &n))
                        && self.wb.ocultos.len() < 256
                    {
                        self.wb.ocultos.push(n);
                    }
                }
                WBCTRLA_REMOVEHIDDENDEVICENAME => {
                    let n = cadena(ex, p)?;
                    self.wb.ocultos.retain(|o| !crate::mem::iguales_sin_caja(o, &n));
                }
                WBCTRLA_GETCOPYHOOK | WBCTRLA_GETDELETEHOOK | WBCTRLA_GETTEXTINPUTHOOK | WBCTRLA_GETDISKINFOHOOK => {
                    let v = self.wb.ganchos.get(&(e.tag + 1)).copied().unwrap_or(0);
                    poner(ex, p, v)?
                }
                WBCTRLA_SETCOPYHOOK | WBCTRLA_SETDELETEHOOK | WBCTRLA_SETTEXTINPUTHOOK | WBCTRLA_SETDISKINFOHOOK => {
                    self.wb.ganchos.insert(e.tag, p);
                }
                WBCTRLA_ADDSETUPCLEANUPHOOK => {
                    if p != 0 && !self.wb.limpieza.contains(&p) {
                        self.wb.limpieza.push(p);
                    }
                }
                WBCTRLA_REMSETUPCLEANUPHOOK => self.wb.limpieza.retain(|&g| g != p),
                // Las que no se conocen se dejan (como hace utility con las etiquetas que no son suyas).
                _ => {}
            }
        }
        let _ = en;
        Ok(1)
    }

    /// El puerto propio a donde vuelven los `AppMessage` (sin señal: se miran con `recoger`).
    fn puerto_propio<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<u32, Fallo> {
        if self.wb.puerto != 0 {
            return Ok(self.wb.puerto);
        }
        let p = ex.alloc_mem(msgport::TAMANO, memf::PUBLIC | memf::CLEAR)?;
        if p == 0 {
            return Err(Fallo::SinSitio);
        }
        ex.cambiar_dueno(p, 0);
        e8(ex, p + node::LN_TYPE, nt::MSGPORT)?;
        e8(ex, p + msgport::MP_FLAGS, msgport::PA_IGNORE)?;
        ex.new_list(p + msgport::MP_MSGLIST, nt::MESSAGE)?;
        self.wb.puerto = p;
        Ok(p)
    }

    /// Entrega un `AppMessage` al puerto del objeto App `objeto` (para el Workbench que lo haya): con sus argumentos
    /// (`am_ArgList`), la clase (`AMCLASSICON_...`), dónde estaba el ratón y la hora. El mensaje vuelve, cuando el
    /// programa lo conteste, al puerto propio, y `recoger` lo suelta.
    #[allow(clippy::too_many_arguments)]
    pub fn entregar<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        objeto: u32,
        args: &[Argumento],
        clase: u16,
        x: i16,
        y: i16,
        hora: (u32, u32),
    ) -> Result<Accion, Fallo> {
        let o = self.wb.objetos.iter().find(|o| o.dir == objeto).cloned().ok_or(Fallo::NoReservado { dir: objeto })?;
        let tipo = match o.tipo {
            TipoApp::Ventana => appmessage::AMTYPE_APPWINDOW,
            TipoApp::Icono => appmessage::AMTYPE_APPICON,
            TipoApp::Menu => appmessage::AMTYPE_APPMENUITEM,
            TipoApp::Zona => appmessage::AMTYPE_APPWINDOWZONE,
        };
        let puerto = self.puerto_propio(ex)?;
        let n = args.len().min(4096) as u32;
        let nombres: u32 = args.iter().take(n as usize).map(|a| a.nombre.len() as u32 + 1).sum();
        let largo = appmessage::TAMANO + 2 + n * appmessage::WBARG + nombres;
        let m = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
        if m == 0 {
            return Err(Fallo::SinSitio);
        }
        ex.cambiar_dueno(m, 0);
        let lista = (m + appmessage::TAMANO + 3) & !3;
        let mut t = lista + n * appmessage::WBARG;
        for (k, a) in args.iter().take(n as usize).enumerate() {
            let w = lista + k as u32 * appmessage::WBARG;
            e32(ex, w, a.cerrojo)?;
            e32(ex, w + 4, t)?;
            escribir(ex, t, a.nombre)?;
            t += a.nombre.len() as u32 + 1;
        }
        e8(ex, m + node::LN_TYPE, nt::MESSAGE)?;
        e32(ex, m + message::MN_REPLYPORT, puerto)?;
        e16(ex, m + message::MN_LENGTH, appmessage::TAMANO as u16)?;
        e16(ex, m + appmessage::AM_TYPE, tipo)?;
        e32(ex, m + appmessage::AM_USERDATA, o.datos)?;
        e32(ex, m + appmessage::AM_ID, o.id)?;
        e32(ex, m + appmessage::AM_NUMARGS, n)?;
        e32(ex, m + appmessage::AM_ARGLIST, if n == 0 { 0 } else { lista })?;
        e16(ex, m + appmessage::AM_VERSION, appmessage::AM_VERSION_ACTUAL)?;
        e16(ex, m + appmessage::AM_CLASS, clase)?;
        e16(ex, m + appmessage::AM_MOUSEX, x as u16)?;
        e16(ex, m + appmessage::AM_MOUSEY, y as u16)?;
        e32(ex, m + appmessage::AM_SECONDS, hora.0)?;
        e32(ex, m + appmessage::AM_MICROS, hora.1)?;
        self.wb.fuera.insert(m, largo);
        match ex.put_msg(o.puerto, m) {
            Ok(a) => Ok(a),
            Err(f) => {
                self.wb.fuera.remove(&m);
                let _ = ex.free_mem(m, largo);
                Err(f)
            }
        }
    }

    /// Suelta los `AppMessage` que los programas ya contestaron. Devuelve cuántos.
    pub fn recoger<M: Memoria>(&mut self, ex: &mut Exec<M>) -> usize {
        if self.wb.puerto == 0 {
            return 0;
        }
        let mut n = 0;
        while let Ok(m) = ex.get_msg(self.wb.puerto) {
            if m == 0 {
                break;
            }
            match self.wb.fuera.remove(&m) {
                Some(largo) => {
                    let _ = ex.free_mem(m, largo);
                    n += 1;
                }
                None => self.avisar(ex, "(workbench: respuesta)", Fallo::MensajeMuerto { dir: m }),
            }
        }
        n
    }
}

impl ObjetoApp {
    fn nuevo(tipo: TipoApp, id: u32, datos: u32, puerto: u32) -> ObjetoApp {
        ObjetoApp {
            dir: 0,
            tipo,
            id,
            datos,
            ventana: 0,
            puerto,
            texto: Vec::new(),
            icono: 0,
            tarea: 0,
            caja: [0; 4],
            relativa: [false; 4],
            gancho: 0,
            tecla: 0,
            padre: 0,
        }
    }
}
