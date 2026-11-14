//! `CreateNewProc` y `CreateProc`: lo que dos sabe hacer sin exec. Las etiquetas (`NP_*` de `dos/dostags.h`) se leen
//! y se comprueban aquí; la estructura `Process` y la tarea las monta la caja (que tiene exec), con las ayudas de
//! abajo para los ficheros y cerrojos del proceso nuevo.
//!
//! Lo que AmigaOS no hace y esta sí: las listas de etiquetas se recorren con tope (un `TAG_MORE` en círculo no
//! cuelga), cada puntero se comprueba, y una etiqueta absurda da un aviso y 0 en vez de un proceso a medio hacer.

use alloc::vec::Vec;

use crate::{Caja, Dos, E, Fallo, Flujo, SistemaArchivos};

/// `TAG_USER + 1000` (`NP_Dummy`).
pub const NP_DUMMY: u32 = 0x8000_0000 + 1000;
pub const NP_SEGLIST: u32 = NP_DUMMY + 1;
pub const NP_FREESEGLIST: u32 = NP_DUMMY + 2;
pub const NP_ENTRY: u32 = NP_DUMMY + 3;
pub const NP_INPUT: u32 = NP_DUMMY + 4;
pub const NP_OUTPUT: u32 = NP_DUMMY + 5;
pub const NP_CLOSEINPUT: u32 = NP_DUMMY + 6;
pub const NP_CLOSEOUTPUT: u32 = NP_DUMMY + 7;
pub const NP_ERROR: u32 = NP_DUMMY + 8;
pub const NP_CLOSEERROR: u32 = NP_DUMMY + 9;
pub const NP_CURRENTDIR: u32 = NP_DUMMY + 10;
pub const NP_STACKSIZE: u32 = NP_DUMMY + 11;
pub const NP_NAME: u32 = NP_DUMMY + 12;
pub const NP_PRIORITY: u32 = NP_DUMMY + 13;
pub const NP_CONSOLETASK: u32 = NP_DUMMY + 14;
pub const NP_WINDOWPTR: u32 = NP_DUMMY + 15;
pub const NP_HOMEDIR: u32 = NP_DUMMY + 16;
pub const NP_COPYVARS: u32 = NP_DUMMY + 17;
pub const NP_CLI: u32 = NP_DUMMY + 18;
pub const NP_PATH: u32 = NP_DUMMY + 19;
pub const NP_COMMANDNAME: u32 = NP_DUMMY + 20;
pub const NP_ARGUMENTS: u32 = NP_DUMMY + 21;
pub const NP_NOTIFYONDEATH: u32 = NP_DUMMY + 22;
pub const NP_SYNCHRONOUS: u32 = NP_DUMMY + 23;
pub const NP_EXITCODE: u32 = NP_DUMMY + 24;
pub const NP_EXITDATA: u32 = NP_DUMMY + 25;

const TAG_DONE: u32 = 0;
const TAG_IGNORE: u32 = 1;
const TAG_MORE: u32 = 2;
const TAG_SKIP: u32 = 3;

/// Tope de etiquetas que se miran en una lista (con sus `TAG_MORE`): más es una lista en círculo o basura.
pub const MAX_ETIQUETAS: u32 = 4096;
/// La pila de serie (`NP_StackSize`, dostags.h) y sus límites en la caja.
pub const PILA_DE_SERIE: u32 = 4000;
pub const PILA_MINIMA: u32 = 1024;
pub const PILA_MAXIMA: u32 = 64 << 20;
/// El nombre de serie (`NP_Name`) y lo más largo que se copia.
pub const NOMBRE_DE_SERIE: &[u8] = b"New Process";
pub const MAX_NOMBRE: usize = 255;
/// Lo más largo que se copia de `NP_Arguments`.
pub const MAX_ARGUMENTOS: usize = 4096;

/// Lo que pide `CreateNewProc`, ya comprobado. Lo que no se dio va con su valor de serie (dostags.h).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proceso {
    /// De dónde sale el código: `NP_Seglist` (BPTR) o `NP_Entry`. Uno de los dos, y solo uno.
    pub seglist: u32,
    pub entrada: u32,
    /// `NP_FreeSeglist`: soltar la lista de segmentos al acabar (de serie, no).
    pub soltar_seglist: bool,
    /// `NP_Input`, `NP_Output`, `NP_Error`: `None` es el de serie (`NIL:` los dos primeros; nada el de errores).
    pub entrada_fh: Option<u32>,
    pub salida_fh: Option<u32>,
    pub error_fh: Option<u32>,
    /// Cerrarlos al acabar: de serie, entrada y salida sí, errores no.
    pub cerrar_entrada: bool,
    pub cerrar_salida: bool,
    pub cerrar_error: bool,
    /// `NP_CurrentDir`, `NP_HomeDir`: `None` es una copia de los de quien llama.
    pub cajon: Option<u32>,
    pub cajon_programa: Option<u32>,
    pub pila: u32,
    /// El nombre (copiado, sin el 0).
    pub nombre: Vec<u8>,
    /// `NP_Priority`: `None` es la de quien llama.
    pub prioridad: Option<i8>,
    pub tarea_consola: Option<u32>,
    pub ventana: Option<u32>,
    pub copiar_variables: bool,
    pub cli: bool,
    pub nombre_orden: Option<Vec<u8>>,
    /// `NP_Arguments` (copiados, sin el 0).
    pub argumentos: Option<Vec<u8>>,
    pub al_salir: u32,
    pub datos_al_salir: u32,
    /// Etiquetas que no se conocen (se ignoran, como dos) o no hacen nada aquí (`NP_Path`, `NP_NotifyOnDeath`,
    /// `NP_Synchronous`, que tampoco hacen nada en dos 40).
    pub ignoradas: u32,
}

/// Lo que puede salir mal al leer las etiquetas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MalProceso {
    /// La lista no se puede leer (puntero fuera, en la página 0, impar) o se pasa del tope.
    Lista(Fallo),
    /// Sin `NP_Seglist` ni `NP_Entry`, o con los dos.
    SinCodigo,
    /// Una entrada impar, en la página 0 o fuera de la memoria.
    EntradaMala { dir: u32 },
    /// Una pila de más de `PILA_MAXIMA`.
    PilaAbsurda { bytes: u32 },
}

fn leer32<C: Caja>(c: &C, d: u32) -> Result<u32, Fallo> {
    Dos::<crate::memfs::MemFs>::l32(c, d).map_err(|e| match e {
        E::Mal(f) => f,
        E::Io(_) => Fallo::FueraDeMemoria { dir: d, largo: 4 },
    })
}

fn cadena<C: Caja>(c: &C, d: u32, max: usize) -> Result<Vec<u8>, Fallo> {
    let mut v = Dos::<crate::memfs::MemFs>::cadena(c, d).map_err(|e| match e {
        E::Mal(f) => f,
        E::Io(_) => Fallo::CadenaSinFin { dir: d },
    })?;
    v.truncate(max);
    Ok(v)
}

/// Lee una lista de etiquetas de `CreateNewProc` (`tags`, en A1/D1). Los textos se copian (nombre, argumentos);
/// los punteros que se guardan tal cual (ficheros, cerrojos, ventana) los comprueba quien los use.
pub fn leer_etiquetas<C: Caja>(c: &C, tags: u32) -> Result<Proceso, MalProceso> {
    let mut p = Proceso {
        pila: PILA_DE_SERIE,
        nombre: NOMBRE_DE_SERIE.to_vec(),
        cerrar_entrada: true,
        cerrar_salida: true,
        copiar_variables: true,
        ..Default::default()
    };
    let mut hay_seglist = false;
    let mut hay_entrada = false;
    let mut d = tags;
    let mut vistas = 0u32;
    loop {
        if d == 0 {
            if vistas == 0 {
                // CreateNewProc(NULL): ni código.
                return Err(MalProceso::SinCodigo);
            }
            break;
        }
        if d & 1 != 0 {
            return Err(MalProceso::Lista(Fallo::FueraDeMemoria { dir: d, largo: 8 }));
        }
        vistas += 1;
        if vistas > MAX_ETIQUETAS {
            return Err(MalProceso::Lista(Fallo::FueraDeMemoria { dir: d, largo: 8 }));
        }
        let etiqueta = leer32(c, d).map_err(MalProceso::Lista)?;
        let dato = leer32(c, d.wrapping_add(4)).map_err(MalProceso::Lista)?;
        let siguiente = d.checked_add(8).ok_or(MalProceso::Lista(Fallo::FueraDeMemoria { dir: d, largo: 8 }))?;
        match etiqueta {
            TAG_DONE => break,
            TAG_IGNORE => {}
            TAG_MORE => {
                d = dato;
                if d == 0 {
                    break;
                }
                continue;
            }
            TAG_SKIP => {
                let saltar = dato.min(MAX_ETIQUETAS).saturating_mul(8);
                vistas = vistas.saturating_add(dato.min(MAX_ETIQUETAS));
                d = siguiente
                    .checked_add(saltar)
                    .ok_or(MalProceso::Lista(Fallo::FueraDeMemoria { dir: siguiente, largo: saltar }))?;
                continue;
            }
            NP_SEGLIST => {
                p.seglist = dato;
                hay_seglist = true;
            }
            NP_FREESEGLIST => p.soltar_seglist = dato != 0,
            NP_ENTRY => {
                p.entrada = dato;
                hay_entrada = true;
            }
            NP_INPUT => p.entrada_fh = Some(dato),
            NP_OUTPUT => p.salida_fh = Some(dato),
            NP_CLOSEINPUT => p.cerrar_entrada = dato != 0,
            NP_CLOSEOUTPUT => p.cerrar_salida = dato != 0,
            NP_ERROR => p.error_fh = Some(dato),
            NP_CLOSEERROR => p.cerrar_error = dato != 0,
            NP_CURRENTDIR => p.cajon = Some(dato),
            NP_HOMEDIR => p.cajon_programa = Some(dato),
            NP_STACKSIZE => {
                if dato > PILA_MAXIMA {
                    return Err(MalProceso::PilaAbsurda { bytes: dato });
                }
                p.pila = dato.max(PILA_MINIMA);
            }
            NP_NAME => {
                // Un nombre que no se lee se queda en el de serie (y la caja avisa por el puntero malo).
                if dato != 0 {
                    p.nombre = cadena(c, dato, MAX_NOMBRE).map_err(MalProceso::Lista)?;
                }
            }
            NP_PRIORITY => p.prioridad = Some((dato as i32).clamp(-128, 127) as i8),
            NP_CONSOLETASK => p.tarea_consola = Some(dato),
            NP_WINDOWPTR => p.ventana = Some(dato),
            NP_COPYVARS => p.copiar_variables = dato != 0,
            NP_CLI => p.cli = dato != 0,
            NP_COMMANDNAME => {
                if dato != 0 {
                    p.nombre_orden = Some(cadena(c, dato, MAX_NOMBRE).map_err(MalProceso::Lista)?);
                }
            }
            NP_ARGUMENTS => {
                if dato != 0 {
                    p.argumentos = Some(cadena(c, dato, MAX_ARGUMENTOS).map_err(MalProceso::Lista)?);
                }
            }
            NP_EXITCODE => p.al_salir = dato,
            NP_EXITDATA => p.datos_al_salir = dato,
            _ => p.ignoradas += 1,
        }
        d = siguiente;
    }
    if hay_seglist == hay_entrada {
        return Err(MalProceso::SinCodigo);
    }
    if hay_seglist {
        // La lista de segmentos es un BPTR; el código del primer segmento empieza 4 bytes detrás de él.
        if p.seglist == 0 || p.seglist > 0x3FFF_FFFF {
            return Err(MalProceso::EntradaMala { dir: p.seglist });
        }
        p.entrada = (p.seglist << 2).wrapping_add(4);
    } else {
        p.soltar_seglist = false;
    }
    if p.entrada & 1 != 0 || p.entrada < crate::PAGINA_CERO || c.leer16(p.entrada).is_none() {
        return Err(MalProceso::EntradaMala { dir: p.entrada });
    }
    Ok(p)
}

impl<F: SistemaArchivos> Dos<F> {
    /// `Open("NIL:")` para un proceso nuevo: el BPTR de su `FileHandle`.
    pub fn abrir_nil<C: Caja>(&mut self, c: &mut C) -> Result<u32, u32> {
        self.nuevo_fichero(c, Flujo::Nil).map(|d| d >> 2).map_err(|e| match e {
            E::Io(x) => x,
            E::Mal(f) => f.codigo(),
        })
    }

    /// `DupLock`, para el cajón de un proceso nuevo. `DupLock(0)` es 0.
    pub fn duplicar_cerrojo<C: Caja>(&mut self, c: &mut C, bptr: u32) -> Result<u32, u32> {
        self.dup_lock(c, bptr).map_err(|e| match e {
            E::Io(x) => x,
            E::Mal(f) => f.codigo(),
        })
    }

    /// Cierra un fichero de dos por su BPTR, sea de quien sea (lo que un proceso pidió cerrar al acabar). Nada si no
    /// es un fichero de dos.
    pub fn cerrar_bptr<C: Caja>(&mut self, c: &mut C, bptr: u32) {
        if let Ok(d) = self.fichero(bptr) {
            let _ = self.cerrar_fichero(c, d);
        }
    }

    /// Suelta un cerrojo de dos por su BPTR (el cajón de un proceso que acaba). Nada si no es suyo o es de una
    /// asignación.
    pub fn soltar_bptr<C: Caja>(&mut self, c: &mut C, bptr: u32) {
        if let Ok((d, x)) = self.cerrojo(bptr)
            && !x.de_asignacion
        {
            self.soltar_cerrojo(c, d);
        }
    }

    /// Si `bptr` es un fichero abierto de dos.
    pub fn es_fichero(&self, bptr: u32) -> bool {
        self.fichero(bptr).is_ok()
    }

    /// Si `bptr` es un cerrojo de dos (0, la raíz, también vale).
    pub fn es_cerrojo(&self, bptr: u32) -> bool {
        bptr == 0 || self.cerrojo(bptr).is_ok()
    }
}
