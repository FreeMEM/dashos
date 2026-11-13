//! graphics.library e intuition.library nativas de la caja Amiga (S4 §3, S23, S9).
//!
//! Como exec (`caja/exec`), están escritas en Rust con la interfaz del 68k: `struct RastPort`, `BitMap`,
//! `Window`, `Screen`, `IntuiMessage`... viven en la memoria de la caja, en big-endian, porque los programas Amiga
//! las leen y las escriben a mano. Cada función se llama por la línea A (`$A0FF n RTS` en la tabla de saltos): la
//! caja recibe `Accion::Ajena` de exec y pasa el número a `CajaGrafica::llamar`.
//!
//! Lo que no está en la memoria del 68k:
//! - **Los píxeles de los mapas nativos** (`Lienzo`): ARGB de 32 bits, lo que espera el compositor. Son los de
//!   cada ventana y los de `AllocBitMap` de más de 8 planos o con un amigo nativo. Los mapas que monta el propio
//!   programa (`InitBitMap` y `AllocRaster`) son planares, en su memoria, y se dibujan bit a bit como en el chipset.
//! - **Las paletas**: cada `ColorMap` tiene su tabla de colores ARGB; las plumas se traducen al dibujar.
//! - **Las ventanas**: cada `struct Window` es una ventana del compositor (la idea de Newbench, S4 §3), con su
//!   búfer propio. El puente es el rasgo `Compositor`: la caja lo implementa sobre Wayland.
//!
//! Escrito desde la documentación pública: el ROM Kernel Reference Manual (Libraries: graphics, intuition), los
//! autodocs de graphics.library e intuition.library y los includes y `.fd` del NDK como especificación de la
//! interfaz. No se ha leído ni copiado código de AmigaOS, AROS, UAE ni Amiberry, ni nada de la ROM: la fuente
//! topaz de 8 puntos está dibujada aquí (`fuente.rs`).
//!
//! Cómo se enchufa: `docs/bitacoras/caja-amiga-intuition.md`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod compositor;
pub mod estructuras;
pub mod lienzo;
pub mod lvo;
pub mod teclado;

mod acceso;
mod despacho;
mod dibujo;
mod fuente;
mod gadtools;
mod graphics;
mod idcmp;
mod intuition;
mod marco;
mod menus;
mod requester;
mod tema;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_exec::{Exec, Memoria};

pub use compositor::{Compositor, CompositorDePrueba, Evento, NuevaVentana, Superficie, Tokens};
pub use fuente::{ALTO as FUENTE_ALTO, ANCHO as FUENTE_ANCHO, BASE as FUENTE_BASE};
pub use lienzo::{Lienzo, Rect};

/// Lo que puede salir mal en una llamada. Ninguno tumba la caja: se apunta como aviso y la función devuelve lo
/// que devolvería al fallar (0, o lo que diga su autodoc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallo {
    /// Un fallo de memoria o de exec (fuera de la memoria, página 0, puntero nulo...).
    Exec(caja_exec::Fallo),
    /// Un `RastPort` que no se puede leer.
    RastPortMalo { dir: u32 },
    /// Un `BitMap` imposible: sin memoria detrás, más de 8 planos sin ser nativo, filas que no caben.
    BitMapMalo { dir: u32 },
    /// Un tamaño negativo, cero o demasiado grande.
    TamanoMalo { ancho: i32, alto: i32 },
    /// Una ventana que intuition no tiene abierta (o ya cerrada).
    VentanaDesconocida { dir: u32 },
    /// Una pantalla que no existe.
    PantallaDesconocida { dir: u32 },
    /// Una `TextFont` que no se puede usar.
    FuenteMala { dir: u32 },
    /// Un `ColorMap` que graphics no conoce.
    ColorMapMalo { dir: u32 },
    /// Una lista de etiquetas que no acaba (o en círculo).
    EtiquetasMalas { dir: u32 },
    /// El compositor no ha podido crear la ventana.
    SinCompositor,
    /// No hay memoria en la caja.
    SinMemoria,
    /// `UnlockPubScreen` sin su `LockPubScreen`.
    DesbloqueoDeMas { dir: u32 },
    /// `FreeBitMap` (o parecido) de algo que no se reservó así.
    NoReservado { dir: u32 },
    /// Una función que aún no está hecha.
    NoImplementada { n: u16 },
    /// Algo que el Amiga deja pasar pero suele ser un error.
    Sospechoso { dir: u32 },
}

impl From<caja_exec::Fallo> for Fallo {
    fn from(f: caja_exec::Fallo) -> Fallo {
        Fallo::Exec(f)
    }
}

/// Un fallo apuntado, con la tarea que lo causó y la función. Va al mismo registro que los de exec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aviso {
    pub tarea: u32,
    pub funcion: &'static str,
    pub fallo: Fallo,
}

/// Dónde empiezan los números nativos de cada biblioteca. Tienen que ser mayores que 255 (los de exec) y no
/// chocar con los de otras bibliotecas nativas (dos...).
#[derive(Clone, Copy, Debug)]
pub struct Numeros {
    pub graphics: u16,
    pub intuition: u16,
    pub gadtools: u16,
}

impl Default for Numeros {
    fn default() -> Numeros {
        Numeros { graphics: 0x0400, intuition: 0x0600, gadtools: 0x0A00 }
    }
}

const MAX_AVISOS: usize = 1024;

/// graphics.library e intuition.library de una caja, con su puente al compositor.
pub struct CajaGrafica<C: Compositor> {
    /// El puente con el compositor (Wayland en la caja; `CompositorDePrueba` en las pruebas).
    pub comp: C,
    numeros: Numeros,
    gfx: graphics::Estado,
    intui: intuition::Estado,
    gt: gadtools::Estado,
    menus: menus::Estado,
    /// La versión del tema de DashOS con la que se pintaron las ventanas (`Compositor::version_tema`): si cambia, se
    /// repintan (tema.rs).
    version_tema: u32,
    /// Los requesters de `EasyRequestArgs` abiertos (uno por tarea) y sus títulos (ventana, cadena, largo).
    requesters: Vec<requester::Requester>,
    requester_titulos: Vec<(u32, u32, u32)>,
    avisos: Vec<Aviso>,
}

impl<C: Compositor> CajaGrafica<C> {
    /// Crea graphics.library e intuition.library en la caja (con `crear_biblioteca_nativa` de exec), la fuente
    /// topaz/8, la paleta y la pantalla de Workbench, del tamaño del escritorio que diga el compositor.
    pub fn instalar<M: Memoria>(ex: &mut Exec<M>, comp: C, numeros: Numeros) -> Result<CajaGrafica<C>, Fallo> {
        let mut c = CajaGrafica {
            comp,
            numeros,
            gfx: graphics::Estado::default(),
            intui: intuition::Estado::default(),
            gt: gadtools::Estado::default(),
            menus: menus::Estado::default(),
            version_tema: 0,
            requesters: Vec::new(),
            requester_titulos: Vec::new(),
            avisos: Vec::new(),
        };
        c.instalar_graphics(ex)?;
        c.instalar_intuition(ex)?;
        c.instalar_gadtools(ex)?;
        Ok(c)
    }

    /// El número nativo de una función de gadtools.library, por su nombre.
    pub fn numero_gadtools(&self, nombre: &str) -> Option<u16> {
        lvo::GADTOOLS.iter().position(|(n, _)| *n == nombre).map(|i| self.numeros.gadtools + i as u16)
    }

    /// El número nativo de una función de graphics, por su nombre.
    pub fn numero_graphics(&self, nombre: &str) -> Option<u16> {
        lvo::graphics(nombre).map(|i| self.numeros.graphics + i as u16)
    }

    /// El número nativo de una función de intuition, por su nombre.
    pub fn numero_intuition(&self, nombre: &str) -> Option<u16> {
        lvo::intuition(nombre).map(|i| self.numeros.intuition + i as u16)
    }

    /// La base de graphics.library (`GfxBase`).
    pub fn gfx_base(&self) -> u32 {
        self.gfx.base
    }

    /// La base de intuition.library (`IntuitionBase`).
    pub fn intuition_base(&self) -> u32 {
        self.intui.base
    }

    /// La `struct Screen` del Workbench.
    pub fn workbench(&self) -> u32 {
        self.intui.wb.dir
    }

    /// La fuente topaz/8 (`struct TextFont`).
    pub fn topaz(&self) -> u32 {
        self.gfx.topaz
    }

    pub fn avisos(&self) -> &[Aviso] {
        &self.avisos
    }

    /// Saca los avisos apuntados (para el registro del Enforcer o Forja).
    pub fn tomar_avisos(&mut self) -> Vec<Aviso> {
        core::mem::take(&mut self.avisos)
    }

    /// El lienzo nativo de un `BitMap`, si lo es (para las pruebas y para el compositor).
    pub fn lienzo(&self, bm: u32) -> Option<&Lienzo> {
        self.gfx.lienzos.get(&bm)
    }

    /// Las ventanas abiertas (`struct Window`), en el orden en que se abrieron.
    pub fn ventanas(&self) -> Vec<u32> {
        let mut v: Vec<(u64, u32)> = self.intui.ventanas.iter().map(|(d, w)| (w.orden, *d)).collect();
        v.sort();
        v.into_iter().map(|(_, d)| d).collect()
    }

    /// La superficie del compositor de una ventana.
    pub fn superficie(&self, ventana: u32) -> Option<Superficie> {
        self.intui.ventanas.get(&ventana).map(|v| v.superficie)
    }

    /// Ventanas cerradas que esperan a que el programa conteste sus mensajes.
    pub fn cerradas_pendientes(&self) -> usize {
        self.intui.cerradas.len()
    }

    /// La hora que da `CurrentTime` y llevan los mensajes (segundos y microsegundos desde 1978). La pone la caja.
    pub fn poner_hora(&mut self, segundos: u32, micros: u32) {
        self.intui.hora = (segundos, micros);
    }

    /// El aspecto de los controles de una ventana (S26): el de DashOS si su programa va "como DashOS"; si no, el
    /// clásico.
    pub(crate) fn tema_de(&self, w: u32) -> tema::Elegido {
        let programa = self.intui.ventanas.get(&w).map(|v| v.programa.as_str()).unwrap_or("");
        tema::Elegido::de(self.comp.tema_amiga(programa))
    }

    /// El nombre del programa de una tarea, para las excepciones del aspecto: el de su nodo, sin el camino
    /// ("SYS:C/Dir" da "Dir").
    pub(crate) fn nombre_programa<M: Memoria>(ex: &Exec<M>, tarea: u32) -> alloc::string::String {
        use crate::acceso::Acceso;
        let d = ex.l32(tarea.wrapping_add(caja_exec::estructuras::node::LN_NAME)).unwrap_or(0);
        let n = if d != 0 { ex.cadena(d).unwrap_or_default() } else { Vec::new() };
        let desde = n.iter().rposition(|&c| c == b':' || c == b'/').map_or(0, |k| k + 1);
        n[desde..].iter().take(255).map(|&c| c as char).collect()
    }

    pub(crate) fn avisar<M: Memoria>(&mut self, ex: &Exec<M>, funcion: &'static str, fallo: Fallo) {
        if self.avisos.len() >= MAX_AVISOS {
            self.avisos.remove(0);
        }
        self.avisos.push(Aviso { tarea: ex.tarea_actual(), funcion, fallo });
    }
}

/// Un mapa de direcciones a algo, para las tablas nativas.
pub(crate) type Tabla<T> = BTreeMap<u32, T>;
