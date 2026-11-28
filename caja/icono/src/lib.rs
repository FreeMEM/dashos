//! icon.library (45) y workbench.library (45) nativas de la caja Amiga (S4).
//!
//! Las de un AmigaOS 3.2 llaman en su `Init` a una privada de exec y la caja no las puede cargar; estas están escritas
//! aquí, en Rust, con la interfaz del 68k: los `DiskObject` que reciben los programas viven en la memoria de la caja
//! con la disposición exacta de `workbench/workbench.h` (un bloque por icono, que suelta `FreeDiskObject`), y los
//! tooltypes son un vector de punteros a cadenas terminado en nulo.
//!
//! - icon.library: leer y escribir `.info` (`info.rs`, `color.rs`): el clásico, NewIcons, el de color de OS 3.5
//!   (`FORM ICON`) y el PNG de OS4 y 3.2; los tooltypes (`tooltypes.rs`); los iconos de serie (`serie.rs`); las
//!   funciones de la versión 44 (`GetIconTagList`, `PutIconTagList`, `DupDiskObjectA`, `IconControlA`...) y pintar
//!   (`DrawIconStateA`, `GetIconRectangleA`: `dibujo.rs`).
//! - workbench.library (`wb.rs`): la caja no tiene Workbench con iconos. Los objetos App (ventanas, iconos, menús y
//!   zonas) se dan y se llevan en una tabla, para que el día que lo haya se entreguen sus `AppMessage`; lo que
//!   necesita un Workbench de verdad devuelve lo que dice el autodoc cuando no lo encuentra.
//!
//! Los ficheros, las llamadas a graphics y lanzar programas no son cosa de estas bibliotecas: los da quien las
//! enchufa por el rasgo `Entorno` (`por_dos.rs` lo hace sobre la dos.library de la caja, con su guardia).
//!
//! Escrito desde la documentación pública: los autodocs de icon.library y workbench.library (3.1, y los de la 44 en
//! adelante), los includes y los `.fd` del NDK 3.2 y la descripción pública del formato de los iconos (Dirk Stöcker,
//! 2002), con ideas del lector de DashOS (`iconos-cajas`). No se ha leído código de AmigaOS, AROS, UAE ni Amiberry.
//!
//! Lo que manda el 68k se comprueba (el Enforcer de S3): un puntero fuera, nulo o en la página 0, un `DiskObject`
//! que no es de icon.library, liberar dos veces, un `.info` roto o con medidas absurdas dan un aviso o el error de dos
//! (en `IoErr()` y en `ICONA_ErrorCode`), nunca un pánico. Si una tarea muere, lo suyo se olvida (exec suelta su
//! memoria).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod color;
pub mod dibujo;
pub mod diskobj;
pub mod info;
pub mod mem;
pub mod por_dos;
pub mod serie;
pub mod tooltypes;
pub mod wb;

mod icon;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_exec::{Accion, Aviso, Exec, Fallo, Memoria, Registros};

pub use caja_dos::errores;
pub use icon::{Global, Icono, nombre_info, oscurecer, tags};

/// Las funciones de icon.library desde -30, en el orden de `icon_lib.fd` (NDK 3.2).
pub const FUNCIONES_ICON: [&str; 30] = [
    "iconPrivate1",
    "iconPrivate2",
    "iconPrivate3",
    "iconPrivate4",
    "FreeFreeList",
    "iconPrivate5",
    "iconPrivate6",
    "AddFreeList",
    "GetDiskObject",
    "PutDiskObject",
    "FreeDiskObject",
    "FindToolType",
    "MatchToolValue",
    "BumpRevision",
    "FreeAlloc",
    "GetDefDiskObject",
    "PutDefDiskObject",
    "GetDiskObjectNew",
    "DeleteDiskObject",
    "FreeFree",
    "DupDiskObjectA",
    "IconControlA",
    "DrawIconStateA",
    "GetIconRectangleA",
    "NewDiskObject",
    "GetIconTagList",
    "PutIconTagList",
    "LayoutIconA",
    "ChangeToSelectedIconColor",
    "BumpRevisionLength",
];

/// Las funciones de workbench.library desde -30, en el orden de `wb_lib.fd` (NDK 3.2).
pub const FUNCIONES_WB: [&str; 19] = [
    "UpdateWorkbench",
    "wbPrivate1",
    "wbPrivate2",
    "AddAppWindowA",
    "RemoveAppWindow",
    "AddAppIconA",
    "RemoveAppIcon",
    "AddAppMenuItemA",
    "RemoveAppMenuItem",
    "wbPrivate3",
    "WBInfo",
    "OpenWorkbenchObjectA",
    "CloseWorkbenchObjectA",
    "WorkbenchControlA",
    "AddAppWindowDropZoneA",
    "RemoveAppWindowDropZone",
    "ChangeWorkbenchSelectionA",
    "MakeWorkbenchObjectVisibleA",
    "WhichWorkbenchObjectA",
];

pub const VERSION_ICON: u16 = 45;
pub const VERSION_WB: u16 = 45;
/// Cuántos números nativos ocupan las dos: los de icon.library y, detrás, los de workbench.library.
pub const NUMEROS: u16 = (FUNCIONES_ICON.len() + FUNCIONES_WB.len()) as u16;

const MAX_AVISOS: usize = 1024;

/// Qué es un objeto del disco (para elegir su icono de serie).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clase {
    /// Un volumen (`DH0:`).
    Disco,
    Cajon,
    /// Un fichero ejecutable (empieza por `HUNK_HEADER`).
    Programa,
    /// Cualquier otro fichero.
    Proyecto,
}

/// Lo que las bibliotecas piden a la caja. Los nombres son del Amiga (Latin-1), relativos al cajón actual de la tarea
/// en marcha, como en `Open()` de dos. Los errores, códigos de dos (`ERROR_OBJECT_NOT_FOUND`...).
pub trait Entorno<M: Memoria> {
    /// El fichero entero; `ERROR_OBJECT_TOO_LARGE` si pasa de `tope` bytes.
    fn leer(&mut self, ex: &mut Exec<M>, nombre: &[u8], tope: u32) -> Result<Vec<u8>, u32>;
    /// Crea o reemplaza el fichero con `datos`.
    fn escribir(&mut self, ex: &mut Exec<M>, nombre: &[u8], datos: &[u8]) -> Result<(), u32>;
    /// Borra el fichero.
    fn borrar(&mut self, ex: &mut Exec<M>, nombre: &[u8]) -> Result<(), u32>;
    /// Qué es `nombre` (o `ERROR_OBJECT_NOT_FOUND`).
    fn clase(&mut self, ex: &mut Exec<M>, nombre: &[u8]) -> Result<Clase, u32>;
    /// Pone `IoErr()` de la tarea en marcha.
    fn poner_io_err(&mut self, ex: &mut Exec<M>, codigo: u32);
    /// Ejecuta en el acto la función nativa `n` de otra biblioteca de la caja (graphics, intuition, cybergraphics)
    /// con los registros `r` (A6, su base). Verdad si lo hizo. Sin ella, `DrawIconStateA` no pinta.
    fn nativa(&mut self, _ex: &mut Exec<M>, _n: u16, _r: &mut Registros) -> bool {
        false
    }
    /// `OpenWorkbenchObjectA` de un programa: lanzarlo como desde el Workbench (con un `WBStartup`), con estos
    /// argumentos (cerrojo, nombre; el primero es el propio programa) y esta pila. Sin ella, no se lanza nada y la
    /// función falla con `ERROR_NOT_IMPLEMENTED`.
    fn lanzar(&mut self, _ex: &mut Exec<M>, _args: &[(u32, Vec<u8>)], _pila: u32) -> Result<(), u32> {
        Err(errores::ERROR_NOT_IMPLEMENTED)
    }
}

/// icon.library y workbench.library de una caja.
pub struct Iconos {
    pub(crate) icon_base: u32,
    pub(crate) wb_base: u32,
    /// Número nativo de la primera función de icon.library (`iconPrivate1`); las demás, seguidas.
    primero: u16,
    /// Los `DiskObject` dados (`GetDiskObject`...), por su dirección.
    pub(crate) iconos: BTreeMap<u32, Icono>,
    /// Las `FreeList` de `AddFreeList` y `FreeAlloc`: la tarea que la usó y lo apuntado en ella (dirección, largo).
    pub(crate) listas: BTreeMap<u32, (u32, Vec<(u32, u32)>)>,
    pub(crate) global: Global,
    pub(crate) wb: wb::Workbench,
    avisos: Vec<Aviso>,
}

impl Iconos {
    /// Crea las dos bibliotecas en la caja, con los números nativos `primero..primero + NUMEROS`: icon.library desde
    /// `primero`, workbench.library desde `primero + FUNCIONES_ICON.len()`.
    pub fn instalar<M: Memoria>(ex: &mut Exec<M>, primero: u16) -> Result<Iconos, Fallo> {
        let n_icon: Vec<u16> = (0..FUNCIONES_ICON.len() as u16).map(|i| primero + i).collect();
        let n_wb: Vec<u16> =
            (0..FUNCIONES_WB.len() as u16).map(|i| primero + FUNCIONES_ICON.len() as u16 + i).collect();
        let icon_base = ex.crear_biblioteca_nativa("icon.library", VERSION_ICON, 1, &n_icon, 0)?;
        let wb_base = ex.crear_biblioteca_nativa("workbench.library", VERSION_WB, 1, &n_wb, 0)?;
        Ok(Iconos {
            icon_base,
            wb_base,
            primero,
            iconos: BTreeMap::new(),
            listas: BTreeMap::new(),
            global: Global::default(),
            wb: wb::Workbench::default(),
            avisos: Vec::new(),
        })
    }

    pub fn icon_base(&self) -> u32 {
        self.icon_base
    }

    pub fn wb_base(&self) -> u32 {
        self.wb_base
    }

    pub fn es_mio(&self, n: u16) -> bool {
        n >= self.primero && n < self.primero + NUMEROS
    }

    /// El nombre de la función nativa `n`, si es de estas bibliotecas.
    pub fn nombre(&self, n: u16) -> Option<&'static str> {
        if !self.es_mio(n) {
            return None;
        }
        let k = (n - self.primero) as usize;
        Some(if k < FUNCIONES_ICON.len() { FUNCIONES_ICON[k] } else { FUNCIONES_WB[k - FUNCIONES_ICON.len()] })
    }

    pub fn avisos(&self) -> &[Aviso] {
        &self.avisos
    }

    pub fn tomar_avisos(&mut self) -> Vec<Aviso> {
        core::mem::take(&mut self.avisos)
    }

    pub(crate) fn avisar<M: Memoria>(&mut self, ex: &Exec<M>, funcion: &'static str, fallo: Fallo) {
        if self.avisos.len() >= MAX_AVISOS {
            self.avisos.remove(0);
        }
        self.avisos.push(Aviso { tarea: ex.tarea_actual(), funcion, fallo });
    }

    /// Cuántos `DiskObject` de icon.library hay vivos (para las pruebas y el registro de la caja).
    pub fn iconos_vivos(&self) -> usize {
        self.iconos.len()
    }

    /// Si `d` es un `DiskObject` dado por icon.library y aún no soltado.
    pub fn es_icono(&self, d: u32) -> bool {
        self.iconos.contains_key(&d)
    }

    /// La función nativa `n` (con `A6`, la base de su biblioteca). `Accion::Ajena` si no es de estas. Todas acaban en
    /// el acto: devuelven `Accion::Seguir`.
    pub fn llamar<M: Memoria, E: Entorno<M>>(
        &mut self,
        ex: &mut Exec<M>,
        n: u16,
        r: &mut Registros,
        en: &mut E,
    ) -> Accion {
        let Some(nombre) = self.nombre(n) else { return Accion::Ajena };
        let res = if ((n - self.primero) as usize) < FUNCIONES_ICON.len() {
            self.llamar_icon(ex, nombre, r, en)
        } else {
            self.llamar_wb(ex, nombre, r, en)
        };
        if let Err(f) = res {
            self.avisar(ex, nombre, f);
            r.d[0] = 0;
        }
        Accion::Seguir
    }

    /// Una tarea ha muerto: se olvida lo suyo (exec suelta su memoria): sus iconos, sus listas y sus objetos App.
    pub fn tarea_muerta(&mut self, t: u32) {
        self.iconos.retain(|_, i| i.dueno != t);
        self.listas.retain(|_, l| l.0 != t);
        self.wb.tarea_muerta(t);
    }

    /// Los objetos App vivos (para un Workbench futuro y las pruebas).
    pub fn objetos_app(&self) -> &[wb::ObjetoApp] {
        self.wb.objetos()
    }
}
