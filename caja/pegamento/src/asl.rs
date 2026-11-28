//! asl.library nativa (rama amiga-apps): el pedidor de ficheros de verdad, con la ventana de la caja
//! (`caja_intuition`, `pedidor.rs`) y los cajones leídos con la dos de la caja (con sus asignaciones y su guardia).
//! La del AmigaOS 3.2 del usuario no se puede cargar (usa una función privada de exec en su `Init`): sin esta, MultiView,
//! Directory Opus, LightWave (cargar un objeto) o Hangar no tienen con qué pedir un fichero.
//!
//! Escrita del autodoc de asl.library y `libraries/asl.h` (NDK 3.2; los desplazamientos de `FileRequester`, sacados
//! con `offsetof` del compilador del 68k). Lo que hay:
//! - `AllocAslRequest`/`AllocFileRequest`, `FreeAslRequest`/`FreeFileRequest`, `AslRequest`/`RequestFile`,
//!   `AbortAslRequest` y `ActivateAslRequest`.
//! - El pedidor de ficheros (`ASL_FileRequest`): `ASLFR_TitleText`, `ASLFR_PositiveText`, `ASLFR_InitialDrawer`,
//!   `ASLFR_InitialFile`, `ASLFR_InitialPattern`, `ASLFR_DoSaveMode`, `ASLFR_DrawersOnly`, `ASLFR_RejectIcons`,
//!   `ASLFR_UserData`. Al aceptar, `fr_Drawer` y `fr_File` (cadenas de la biblioteca, que duran hasta el siguiente
//!   `AslRequest` o `FreeAslRequest`) y `fr_NumArgs` = 0. La selección múltiple (`ASLFR_DoMultiSelect`) da un fichero.
//! - El de modos de pantalla (`ASL_ScreenModeRequest`): la caja tiene un solo modo (el suyo, `MODO_CAJA`, del tamaño
//!   del escritorio, 32 bits): se da ese, sin ventana (no hay nada que elegir).
//! - El de fuentes (`ASL_FontRequest`): aún no (FALSE y un aviso: "falta").

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Accion, Exec, Fallo, Memoria, Registros};
use caja_intuition::AccionAsl;

use crate::memoria::MemoriaCaja;

pub const FUNCIONES: [&str; 10] = [
    "AllocFileRequest",
    "FreeFileRequest",
    "RequestFile",
    "AllocAslRequest",
    "FreeAslRequest",
    "AslRequest",
    "aslPrivate1",
    "aslPrivate2",
    "AbortAslRequest",
    "ActivateAslRequest",
];

const SIGF_SINGLE: u32 = 1 << 4;
const TB: u32 = 0x8008_0000;
const TITLETEXT: u32 = TB + 1;
const INITIALFILE: u32 = TB + 8;
const INITIALDRAWER: u32 = TB + 9;
const INITIALPATTERN: u32 = TB + 10;
const POSITIVETEXT: u32 = TB + 18;
const FLAGS1: u32 = TB + 20;
const DOSAVEMODE: u32 = TB + 44;
const DRAWERSONLY: u32 = TB + 47;
const USERDATA: u32 = TB + 52;
const REJECTICONS: u32 = TB + 60;
const FRF_DOSAVEMODE: u32 = 0x20;
/// `FileRequester` (`libraries/asl.h`).
const FR_FILE: u32 = 4;
const FR_DRAWER: u32 = 8;
const FR_NUMARGS: u32 = 32;
const FR_ARGLIST: u32 = 36;
const FR_USERDATA: u32 = 40;
const FR_PATTERN: u32 = 52;
const FR_TAMANO: u32 = 56;
const SM_TAMANO: u32 = 48;
const FO_TAMANO: u32 = 44;
const MAX_ENTRADAS: usize = 4000;

struct Pedido {
    tipo: u32,
    tamano: u32,
    tarea: u32,
    titulo: Vec<u8>,
    aceptar: Vec<u8>,
    cajon: Vec<u8>,
    fichero: Vec<u8>,
    patron: Vec<u8>,
    solo_cajones: bool,
    sin_iconos: bool,
    /// Las cadenas que se dieron al programa (dirección, largo), para soltarlas.
    cadenas: Vec<(u32, u32)>,
    /// Viendo la lista de volúmenes y asignaciones en vez de un cajón.
    volumenes: bool,
}

#[derive(Default)]
pub struct Asl {
    n0: u16,
    pub base: u32,
    pedidos: BTreeMap<u32, Pedido>,
    /// Lo que no está (para el registro de lo que falta).
    pub faltan: Vec<&'static str>,
}

/// Lo que hace falta de la caja: exec, intuition (la ventana) y la dos (los cajones, con su guardia).
pub(crate) struct EntornoAsl<'a> {
    pub ex: &'a mut Exec<MemoriaCaja>,
    pub gr: &'a mut caja_intuition::CajaGrafica<crate::compositor::CompositorWayland>,
    pub dos: &'a mut caja_dos::Dos<caja_dos::guardia::Guardia<crate::archivos::Fs>>,
    pub consola: &'a mut crate::Consola,
    /// El modo de pantalla de la caja y el tamaño del escritorio.
    pub modo: (u32, u32, u32),
}

impl EntornoAsl<'_> {
    fn listar(&mut self, ruta: &[u8]) -> Result<Vec<(Vec<u8>, bool)>, u32> {
        let mut cd = crate::CajaDos { ex: self.ex, consola: self.consola, sistema: false };
        Ok(self.dos.listar_cajon(&mut cd, ruta, MAX_ENTRADAS)?.into_iter().map(|(n, c, _)| (n, c)).collect())
    }

    fn dispositivos(&mut self) -> Vec<Vec<u8>> {
        self.dos.nombres_de_dispositivos().into_iter().map(|(n, _)| n).collect()
    }
}

/// Una lista de etiquetas (`utility/tagitem.h`): `TAG_DONE`, `TAG_IGNORE`, `TAG_MORE` y `TAG_SKIP`; como mucho 512.
fn leer_etiquetas(ex: &Exec<MemoriaCaja>, mut d: u32) -> Result<Vec<(u32, u32)>, Fallo> {
    let mut v = Vec::new();
    let mut vueltas = 0;
    while d != 0 && vueltas < 512 {
        vueltas += 1;
        let (t, x) = (ex.mem.leer32(d)?, ex.mem.leer32(d + 4)?);
        match t {
            0 => break,
            1 => d += 8,
            2 => d = x,
            3 => d += 8 * (x.min(512) + 1),
            _ => {
                v.push((t, x));
                d += 8;
            }
        }
    }
    Ok(v)
}

/// Un cajón más un nombre, como `AddPart`.
fn unir(cajon: &[u8], nombre: &[u8]) -> Vec<u8> {
    let mut v = cajon.to_vec();
    if !v.is_empty() && !matches!(v.last(), Some(b':') | Some(b'/')) {
        v.push(b'/');
    }
    v.extend_from_slice(nombre);
    v
}

/// El padre de un cajón (como `PathPart` de lo que no acaba en ':'). `None` si ya es la raíz de un volumen.
fn padre(cajon: &[u8]) -> Option<Vec<u8>> {
    let c = cajon.strip_suffix(b"/").unwrap_or(cajon);
    if c.is_empty() || c.ends_with(b":") {
        return None;
    }
    match c.iter().rposition(|&b| b == b'/' || b == b':') {
        Some(i) if c[i] == b':' => Some(c[..=i].to_vec()),
        Some(i) => Some(c[..i].to_vec()),
        None => Some(Vec::new()),
    }
}

/// Un comodín de AmigaDOS sencillo (`#?`, `?` y `*`), sin mirar mayúsculas: para `ASLFR_InitialPattern`.
fn casa(patron: &[u8], nombre: &[u8]) -> bool {
    fn rec(p: &[u8], n: &[u8], prof: u32) -> bool {
        if prof > 64 {
            return false;
        }
        match p {
            [] => n.is_empty(),
            [b'#', b'?', resto @ ..] | [b'*', resto @ ..] => (0..=n.len()).any(|i| rec(resto, &n[i..], prof + 1)),
            [b'?', resto @ ..] => !n.is_empty() && rec(resto, &n[1..], prof + 1),
            [c, resto @ ..] => n.first().is_some_and(|x| x.eq_ignore_ascii_case(c)) && rec(resto, &n[1..], prof + 1),
        }
    }
    patron.is_empty() || rec(patron, nombre, 0)
}

impl Asl {
    pub fn instalar(ex: &mut Exec<MemoriaCaja>, n0: u16) -> Result<Asl, Fallo> {
        let nums: Vec<u16> = (0..FUNCIONES.len() as u16).map(|i| n0 + i).collect();
        let base = ex.crear_biblioteca_nativa("asl.library", 45, 1, &nums, 34)?;
        Ok(Asl { n0, base, ..Default::default() })
    }

    pub fn es_mio(&self, n: u16) -> bool {
        (self.n0..self.n0 + FUNCIONES.len() as u16).contains(&n)
    }

    pub fn tarea_muerta(&mut self, t: u32) {
        self.pedidos.retain(|_, p| p.tarea != t);
    }

    pub(crate) fn llamar(&mut self, en: &mut EntornoAsl, n: u16, r: &mut Registros) -> Result<Accion, Fallo> {
        let f = (n - self.n0) as usize;
        match f {
            // AllocFileRequest() y AllocAslRequest(type, tags)(d0/a0)
            0 | 3 => {
                let (tipo, tags) = if f == 0 { (0, 0) } else { (r.d[0], r.a[0]) };
                r.d[0] = self.alloc(en, tipo, tags)?;
                Ok(Accion::Seguir)
            }
            // FreeFileRequest y FreeAslRequest(requester)(a0)
            1 | 4 => {
                self.free(en, r.a[0])?;
                Ok(Accion::Seguir)
            }
            // RequestFile(fileReq)(a0) y AslRequest(requester, tags)(a0/a1)
            2 | 5 => self.pedir(en, r, if f == 2 { 0 } else { r.a[1] }),
            // AbortAslRequest: el pedidor abierto de ese pedido se cancela.
            8 => {
                if let Some(p) = self.pedidos.get(&r.a[0]) {
                    let t = p.tarea;
                    if en.gr.pedidor_abierto(t) {
                        let _ = en.ex.signal(t, SIGF_SINGLE);
                    }
                }
                Ok(Accion::Seguir)
            }
            9 => Ok(Accion::Seguir),
            _ => {
                self.faltan.push(FUNCIONES[f]);
                r.d[0] = 0;
                Ok(Accion::Seguir)
            }
        }
    }

    fn etiquetas(&mut self, en: &mut EntornoAsl, req: u32, tags: u32) -> Result<(), Fallo> {
        if tags == 0 {
            return Ok(());
        }
        let lista = leer_etiquetas(en.ex, tags)?;
        let ex = &mut *en.ex;
        let cad = |ex: &Exec<MemoriaCaja>, d: u32| -> Vec<u8> {
            if d == 0 { Vec::new() } else { ex.leer_cadena(d).map(|s| s.chars().map(|c| c as u8).take(255).collect()).unwrap_or_default() }
        };
        let Some(p) = self.pedidos.get_mut(&req) else { return Ok(()) };
        for (t, v) in lista {
            match t {
                TITLETEXT => p.titulo = cad(ex, v),
                POSITIVETEXT => p.aceptar = cad(ex, v),
                INITIALDRAWER => p.cajon = cad(ex, v),
                INITIALFILE => p.fichero = cad(ex, v),
                INITIALPATTERN => p.patron = cad(ex, v),
                DOSAVEMODE if v != 0 && p.aceptar.is_empty() => p.aceptar = b"Guardar".to_vec(),
                FLAGS1 if v & FRF_DOSAVEMODE != 0 && p.aceptar.is_empty() => p.aceptar = b"Guardar".to_vec(),
                DRAWERSONLY => p.solo_cajones = v != 0,
                REJECTICONS => p.sin_iconos = v != 0,
                USERDATA if p.tipo == 0 => ex.mem.escribir32(req + FR_USERDATA, v)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn alloc(&mut self, en: &mut EntornoAsl, tipo: u32, tags: u32) -> Result<u32, Fallo> {
        let tamano = match tipo {
            0 => FR_TAMANO,
            1 => FO_TAMANO,
            2 => SM_TAMANO,
            _ => return Ok(0),
        };
        let ex = &mut *en.ex;
        let req = ex.alloc_mem(tamano, memf::PUBLIC | memf::CLEAR)?;
        if req == 0 {
            return Ok(0);
        }
        let t = ex.tarea_actual();
        self.pedidos.insert(
            req,
            Pedido {
                tipo,
                tamano,
                tarea: t,
                titulo: Vec::new(),
                aceptar: Vec::new(),
                cajon: Vec::new(),
                fichero: Vec::new(),
                patron: Vec::new(),
                solo_cajones: false,
                sin_iconos: false,
                cadenas: Vec::new(),
                volumenes: false,
            },
        );
        self.etiquetas(en, req, tags)?;
        if tipo == 0 {
            self.poner_cadenas(en.ex, req)?;
        }
        Ok(req)
    }

    /// `fr_File`, `fr_Drawer` y `fr_Pattern` con lo de ahora (cadenas nuevas; las de antes se sueltan).
    fn poner_cadenas(&mut self, ex: &mut Exec<MemoriaCaja>, req: u32) -> Result<(), Fallo> {
        let Some(p) = self.pedidos.get_mut(&req) else { return Ok(()) };
        for (d, n) in core::mem::take(&mut p.cadenas) {
            let _ = ex.free_mem(d, n);
        }
        for (campo, texto) in [(FR_FILE, p.fichero.clone()), (FR_DRAWER, p.cajon.clone()), (FR_PATTERN, p.patron.clone())] {
            let n = texto.len() as u32 + 1;
            let d = ex.alloc_mem(n.max(2), memf::PUBLIC | memf::CLEAR)?;
            if d == 0 {
                ex.mem.escribir32(req + campo, 0)?;
                continue;
            }
            for (k, b) in texto.iter().enumerate() {
                ex.mem.escribir8(d + k as u32, *b)?;
            }
            p.cadenas.push((d, n.max(2)));
            ex.mem.escribir32(req + campo, d)?;
        }
        ex.mem.escribir32(req + FR_NUMARGS, 0)?;
        ex.mem.escribir32(req + FR_ARGLIST, 0)
    }

    fn free(&mut self, en: &mut EntornoAsl, req: u32) -> Result<(), Fallo> {
        let Some(p) = self.pedidos.remove(&req) else {
            return if req == 0 { Ok(()) } else { Err(Fallo::NoReservado { dir: req }) };
        };
        let ex = &mut *en.ex;
        for (d, n) in p.cadenas {
            let _ = ex.free_mem(d, n);
        }
        ex.free_mem(req, p.tamano)
    }

    /// Lo de un cajón (o los volúmenes), filtrado como pide el programa.
    fn entradas(en: &mut EntornoAsl, p: &Pedido) -> Result<Vec<(Vec<u8>, bool)>, u32> {
        if p.volumenes {
            return Ok(en.dispositivos().into_iter().map(|mut n| {
                n.push(b':');
                (n, true)
            }).collect());
        }
        let v = en.listar(&p.cajon)?;
        Ok(v.into_iter()
            .filter(|(n, c)| *c || (!p.solo_cajones && !(p.sin_iconos && n.ends_with(b".info")) && casa(&p.patron, n)))
            .take(MAX_ENTRADAS)
            .collect())
    }

    /// `AslRequest`: la primera vez abre el pedidor y la tarea espera; cada vez que vuelve (la caja repite la
    /// llamada), mira lo que hizo la persona.
    fn pedir(&mut self, en: &mut EntornoAsl, r: &mut Registros, tags: u32) -> Result<Accion, Fallo> {
        let req = r.a[0];
        let Some(tipo) = self.pedidos.get(&req).map(|p| p.tipo) else {
            r.d[0] = 0;
            return Err(Fallo::NoReservado { dir: req });
        };
        let t = en.ex.tarea_actual();
        match tipo {
            2 => {
                // Un solo modo: el de la caja.
                let (modo, an, al) = en.modo;
                let ex = &mut *en.ex;
                ex.mem.escribir32(req, modo)?;
                ex.mem.escribir32(req + 4, an)?;
                ex.mem.escribir32(req + 8, al)?;
                ex.mem.escribir16(req + 12, 32)?;
                r.d[0] = 1;
                return Ok(Accion::Seguir);
            }
            1 => {
                self.faltan.push("AslRequest (pedidor de fuentes)");
                r.d[0] = 0;
                return Ok(Accion::Seguir);
            }
            _ => {}
        }
        if !en.gr.pedidor_abierto(t) {
            self.etiquetas(en, req, tags)?;
            let Some(p) = self.pedidos.get_mut(&req) else { return Ok(Accion::Seguir) };
            p.tarea = t;
            p.volumenes = false;
            let entradas = match Self::entradas(en, self.pedidos.get(&req).unwrap()) {
                Ok(v) => v,
                Err(_) => {
                    // Un cajón que no está: se empieza en los volúmenes.
                    let p = self.pedidos.get_mut(&req).unwrap();
                    p.volumenes = true;
                    Self::entradas(en, p).unwrap_or_default()
                }
            };
            let p = self.pedidos.get(&req).unwrap();
            let titulo = if p.titulo.is_empty() { b"Elige un fichero".to_vec() } else { p.titulo.clone() };
            let aceptar = if p.aceptar.is_empty() { b"Aceptar".to_vec() } else { p.aceptar.clone() };
            let (cajon, fichero) = (if p.volumenes { Vec::new() } else { p.cajon.clone() }, p.fichero.clone());
            if en.gr.pedidor_abrir(en.ex, t, &titulo, &aceptar, &cajon, &fichero, entradas).is_err() {
                // Sin ventana (sin compositor, sin memoria): como si cancelara.
                r.d[0] = 0;
                return Ok(Accion::Seguir);
            }
            return self.esperar(en);
        }
        // Volvió: ¿qué hizo?
        let Some((accion, cajon_escrito, sel)) = en.gr.pedidor_accion(t) else { return self.esperar(en) };
        let p = self.pedidos.get_mut(&req).unwrap();
        match accion {
            AccionAsl::Aceptar => {
                p.fichero = sel;
                if !p.volumenes {
                    p.cajon = cajon_escrito;
                }
                self.cerrar(en, t);
                self.poner_cadenas(en.ex, req)?;
                r.d[0] = 1;
                Ok(Accion::Seguir)
            }
            AccionAsl::Cancelar => {
                self.cerrar(en, t);
                r.d[0] = 0;
                Ok(Accion::Seguir)
            }
            AccionAsl::Entrar(i) => {
                let antes = (p.cajon.clone(), p.volumenes);
                p.cajon = match i {
                    Some(_) if p.volumenes => sel,
                    Some(_) => unir(&p.cajon, &sel),
                    None => cajon_escrito,
                };
                p.volumenes = false;
                self.mostrar(en, req, t, antes)
            }
            AccionAsl::Padre => {
                let antes = (p.cajon.clone(), p.volumenes);
                match padre(&p.cajon) {
                    Some(c) if !p.volumenes => p.cajon = c,
                    _ => p.volumenes = true,
                }
                self.mostrar(en, req, t, antes)
            }
            AccionAsl::Volumenes => {
                let antes = (p.cajon.clone(), p.volumenes);
                p.volumenes = true;
                self.mostrar(en, req, t, antes)
            }
        }
    }

    /// Pone en el pedidor lo de ahora; si no se puede leer, vuelve a lo de `antes`.
    fn mostrar(&mut self, en: &mut EntornoAsl, req: u32, t: u32, antes: (Vec<u8>, bool)) -> Result<Accion, Fallo> {
        let p = self.pedidos.get_mut(&req).unwrap();
        let entradas = match Self::entradas(en, p) {
            Ok(v) => v,
            Err(_) => {
                (p.cajon, p.volumenes) = antes;
                Self::entradas(en, p).unwrap_or_default()
            }
        };
        let cajon = if p.volumenes { Vec::new() } else { p.cajon.clone() };
        en.gr.pedidor_poner(en.ex, t, &cajon, entradas);
        self.esperar(en)
    }

    fn cerrar(&mut self, en: &mut EntornoAsl, t: u32) {
        en.gr.pedidor_cerrar(en.ex, t);
    }

    /// La tarea espera `SIGF_SINGLE` (la manda el pedidor cuando pasa algo); la caja repetirá la llamada.
    fn esperar(&mut self, en: &mut EntornoAsl) -> Result<Accion, Fallo> {
        en.ex.set_signal(0, SIGF_SINGLE)?;
        en.ex.wait(SIGF_SINGLE)?;
        Ok(Accion::Bloquear)
    }
}
