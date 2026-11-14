//! layers.library y keymap.library nativas, lo poco que piden los programas de hoy (MUI, las bibliotecas de un
//! AmigaOS 3.x de verdad) en una caja donde cada ventana es una superficie del compositor.
//!
//! Escritas de los autodocs (`layers.doc`, `keymap.doc`), el RKRM (Libraries: "Layers Library"; Devices: "Keyboard
//! Device" y "Keymap Library", el formato de `struct KeyMap`) y los `.fd` del NDK 3.2. No se ha leído código de
//! ninguna layers ni keymap.
//!
//! **layers**: las capas las lleva el compositor (una ventana, una superficie), así que lo que en el Amiga reparte la
//! pantalla aquí no tiene qué repartir. Los cerrojos (`LockLayer`, `LockLayerInfo`...) no esperan: en la caja hay una
//! sola tarea 68k a la vez. `InstallClipRegion` guarda la región en la capa y devuelve la de antes; graphics recorta con
//! ella todo lo que se pinta en la capa (`Pintor::region`). `NewLayerInfo`/`DisposeLayerInfo` reservan y sueltan su estructura. Una capa
//! nunca está tapada (`LayerOccluded` = 0).
//!
//! Las capas sueltas (`CreateUpfrontLayer`, `CreateBehindLayer` y sus variantes con gancho; Tachyon pinta así en un
//! mapa fuera de pantalla) son un recorte en su mapa: su `RastPort` dibuja con el (0, 0) en la esquina de la capa y
//! no se sale de ella, como el interior de una ventana `WFLG_GIMMEZEROZERO`. No se tapan entre sí: cada una pinta en
//! su rectángulo (lo normal en un mapa fuera de pantalla, donde hay una sola). `DeleteLayer` las suelta; las de una
//! tarea que acaba, también. `WhichLayer` da la capa de la ventana activa (el compositor sabe dónde está el ratón;
//! la caja, no). Mover, ordenar y los ganchos de relleno de 68k dan el aviso `NoImplementada`.
//!
//! **keymap**: la distribución estadounidense (la de `teclado.rs`), en una `struct KeyMap` de verdad para quien la lea.
//! `MapRawKey` da el carácter Latin-1 de una pulsación o la secuencia CSI de las teclas de cursor y de función;
//! `MapANSI`, al revés. Otra `KeyMap` que pase el programa se usa si es la de la caja; si no, también la de la caja
//! (provisional).

use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Memoria, Registros};

use crate::acceso::{Acceso, palabra, puntero};
use crate::estructuras::{cliprect, ie, layer, layer_info, rastport as rp_, rectangle};
use crate::lienzo::Rect;
use crate::teclado::{Calificadores, caracter_por_defecto, crudo};
use crate::{CajaGrafica, Compositor, Fallo};

/// Las funciones de `layers_lib.fd` desde -30.
pub const LAYERS: [&str; 36] = [
    "InitLayers",
    "CreateUpfrontLayer",
    "CreateBehindLayer",
    "UpfrontLayer",
    "BehindLayer",
    "MoveLayer",
    "SizeLayer",
    "ScrollLayer",
    "BeginUpdate",
    "EndUpdate",
    "DeleteLayer",
    "LockLayer",
    "UnlockLayer",
    "LockLayers",
    "UnlockLayers",
    "LockLayerInfo",
    "SwapBitsRastPortClipRect",
    "WhichLayer",
    "UnlockLayerInfo",
    "NewLayerInfo",
    "DisposeLayerInfo",
    "FattenLayerInfo",
    "ThinLayerInfo",
    "MoveLayerInFrontOf",
    "InstallClipRegion",
    "MoveSizeLayer",
    "CreateUpfrontHookLayer",
    "CreateBehindHookLayer",
    "InstallLayerHook",
    "InstallLayerInfoHook",
    "SortLayerCR",
    "DoHookClipRects",
    "LayerOccluded",
    "HideLayer",
    "ShowLayer",
    "SetLayerInfoBounds",
];

/// Las funciones de `keymap_lib.fd` desde -30.
pub const KEYMAP: [&str; 4] = ["SetKeyMapDefault", "AskKeyMapDefault", "MapRawKey", "MapANSI"];

const INITLAYERS: usize = 0;
const CREATEUPFRONTLAYER: usize = 1;
const CREATEBEHINDLAYER: usize = 2;
const DELETELAYER: usize = 10;
const WHICHLAYER: usize = 17;
const CREATEUPFRONTHOOKLAYER: usize = 26;
const CREATEBEHINDHOOKLAYER: usize = 27;

/// Que los números de arriba sean los de sus nombres en la tabla.
const fn es(i: usize, nombre: &str) -> bool {
    let (a, b) = (LAYERS[i].as_bytes(), nombre.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut k = 0;
    while k < a.len() {
        if a[k] != b[k] {
            return false;
        }
        k += 1;
    }
    true
}
const _: () = assert!(
    es(CREATEUPFRONTLAYER, "CreateUpfrontLayer")
        && es(CREATEBEHINDLAYER, "CreateBehindLayer")
        && es(DELETELAYER, "DeleteLayer")
        && es(WHICHLAYER, "WhichLayer")
        && es(CREATEUPFRONTHOOKLAYER, "CreateUpfrontHookLayer")
        && es(CREATEBEHINDHOOKLAYER, "CreateBehindHookLayer")
        && es(INSTALLCLIPREGION, "InstallClipRegion")
        && es(LAYEROCCLUDED, "LayerOccluded")
        && es(NEWLAYERINFO, "NewLayerInfo")
);
const BEGINUPDATE: usize = 8;
const ENDUPDATE: usize = 9;
const LOCKLAYER: usize = 11;
const UNLOCKLAYER: usize = 12;
const LOCKLAYERS: usize = 13;
const UNLOCKLAYERS: usize = 14;
const LOCKLAYERINFO: usize = 15;
const UNLOCKLAYERINFO: usize = 18;
const NEWLAYERINFO: usize = 19;
const DISPOSELAYERINFO: usize = 20;
const FATTENLAYERINFO: usize = 21;
const THINLAYERINFO: usize = 22;
const INSTALLCLIPREGION: usize = 24;
const SORTLAYERCR: usize = 30;
const LAYEROCCLUDED: usize = 32;

const SETKEYMAPDEFAULT: usize = 0;
const ASKKEYMAPDEFAULT: usize = 1;
const MAPRAWKEY: usize = 2;
const MAPANSI: usize = 3;

/// `struct KeyMap` (`devices/keymap.h`): ocho punteros.
mod keymap {
    pub const LO_TYPES: u32 = 0;
    pub const LO_MAP: u32 = 4;
    pub const LO_CAPSABLE: u32 = 8;
    pub const LO_REPEATABLE: u32 = 12;
    pub const HI_TYPES: u32 = 16;
    pub const HI_MAP: u32 = 20;
    pub const HI_CAPSABLE: u32 = 24;
    pub const HI_REPEATABLE: u32 = 28;
    pub const TAMANO: u32 = 32;
    /// Los tipos de tecla (`KC_NOQUAL`, `KCF_SHIFT`, `KCF_NOP`).
    pub const KC_NOQUAL: u8 = 0;
    pub const KCF_SHIFT: u8 = 1;
    pub const KCF_NOP: u8 = 0x80;
}

/// `struct InputEvent`: lo que lee `MapRawKey`.
mod input_event {
    pub const CLASS: u32 = 4;
    pub const CODE: u32 = 6;
    pub const QUALIFIER: u32 = 8;
    pub const IECLASS_RAWKEY: u8 = 1;
}

/// Una capa suelta: su `RastPort`, su `ClipRect`, su `DamageList` y la tarea que la creó.
#[derive(Clone, Copy)]
pub(crate) struct Suelta {
    rp: u32,
    cliprect: u32,
    dano: u32,
    tarea: u32,
}

/// El estado de las dos.
#[derive(Default)]
pub(crate) struct Estado {
    pub layers: u32,
    pub keymap: u32,
    /// Las capas sueltas: `Layer` → lo suyo.
    sueltas: crate::Tabla<Suelta>,
    /// La `struct KeyMap` de la caja y la que el programa puso por defecto (`SetKeyMapDefault`).
    km_caja: u32,
    km_defecto: u32,
}

/// La secuencia CSI (sin el `$9B`) de las teclas que no escriben un carácter: cursores, F1-F10 y Help (RKRM:
/// Devices, "Console Device", las secuencias de entrada).
fn secuencia(crudo: u8, mayus: bool) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    match crudo {
        0x4C => v.extend_from_slice(if mayus { b"T" } else { b"A" }),
        0x4D => v.extend_from_slice(if mayus { b"S" } else { b"B" }),
        0x4E => v.extend_from_slice(if mayus { b" @" } else { b"C" }),
        0x4F => v.extend_from_slice(if mayus { b" A" } else { b"D" }),
        0x50..=0x59 => {
            let n = (crudo - 0x50) + if mayus { 10 } else { 0 };
            if n >= 10 {
                v.push(b'1');
            }
            v.push(b'0' + n % 10);
            v.push(b'~');
        }
        0x5F => v.extend_from_slice(b"?~"),
        _ => return None,
    }
    Some(v)
}

/// El código de evdev de un código crudo del Amiga (al revés que `teclado::crudo`).
fn evdev_de(c: u8) -> Option<u32> {
    (1..128u32).find(|&e| crudo(e).is_some_and(|(x, _)| x == c))
}

/// El carácter de una tecla cruda con unos calificadores (distribución estadounidense).
fn caracter(c: u8, q: u16) -> Option<u8> {
    caracter_por_defecto(evdev_de(c)?, Calificadores { bits: q })
}

impl<C: Compositor> CajaGrafica<C> {
    pub(crate) fn instalar_capas<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<(), Fallo> {
        let n0 = self.numeros.layers;
        let nums: Vec<u16> = (0..LAYERS.len() as u16).map(|i| n0 + i).collect();
        self.capas.layers = ex.crear_biblioteca_nativa("layers.library", 40, 1, &nums, 34)?;
        let k0 = self.numeros.keymap;
        let nums: Vec<u16> = (0..KEYMAP.len() as u16).map(|i| k0 + i).collect();
        self.capas.keymap = ex.crear_biblioteca_nativa("keymap.library", 40, 1, &nums, 34)?;
        self.capas.km_caja = crear_keymap(ex)?;
        self.capas.km_defecto = self.capas.km_caja;
        Ok(())
    }

    /// La base de layers.library y la de keymap.library.
    pub fn capas_bases(&self) -> (u32, u32) {
        (self.capas.layers, self.capas.keymap)
    }

    /// Si `n` es de layers (`Some((true, i))`) o de keymap (`Some((false, i))`).
    pub(crate) fn es_capas(&self, n: u16) -> Option<(bool, usize)> {
        let (l0, k0) = (self.numeros.layers, self.numeros.keymap);
        if n >= l0 && ((n - l0) as usize) < LAYERS.len() {
            Some((true, (n - l0) as usize))
        } else if n >= k0 && ((n - k0) as usize) < KEYMAP.len() {
            Some((false, (n - k0) as usize))
        } else {
            None
        }
    }

    pub(crate) fn llamar_layers<M: Memoria>(&mut self, ex: &mut Exec<M>, f: usize, r: &mut Registros) -> Result<u32, Fallo> {
        let a = r.a;
        Ok(match f {
            INITLAYERS | ENDUPDATE | LOCKLAYER | UNLOCKLAYER | LOCKLAYERS | UNLOCKLAYERS | LOCKLAYERINFO
            | UNLOCKLAYERINFO | THINLAYERINFO | SORTLAYERCR => 0,
            // Se puede pintar: el compositor guarda lo de la ventana.
            BEGINUPDATE | FATTENLAYERINFO => 1,
            NEWLAYERINFO => ex.alloc_mem(layer_info::TAMANO, memf::PUBLIC | memf::CLEAR)?,
            DISPOSELAYERINFO => {
                if a[0] != 0 {
                    ex.free_mem(a[0], layer_info::TAMANO)?;
                }
                0
            }
            INSTALLCLIPREGION => {
                if a[0] == 0 {
                    return Err(Fallo::Exec(caja_exec::Fallo::PunteroNulo));
                }
                let antes = ex.l32(a[0] + layer::CLIPREGION)?;
                ex.e32(a[0] + layer::CLIPREGION, a[1])?;
                antes
            }
            LAYEROCCLUDED => 0,
            // Create{Upfront,Behind}[Hook]Layer(li, bm, x0, y0, x1, y1, flags, [hook,] bm2)(a0/a1/d0-d4/[a3/]a2).
            CREATEUPFRONTLAYER | CREATEBEHINDLAYER => self.crear_capa(ex, r, 0)?,
            CREATEUPFRONTHOOKLAYER | CREATEBEHINDHOOKLAYER => self.crear_capa(ex, r, r.a[3])?,
            // DeleteLayer(dummy, layer)(a0/a1): TRUE.
            DELETELAYER => {
                self.borrar_capa(ex, a[1])?;
                1
            }
            // WhichLayer(li, x, y)(a0/d0/d1).
            WHICHLAYER => self.capa_activa(),
            _ => return Err(Fallo::NoImplementada { n: self.numeros.layers + f as u16 }),
        })
    }

    /// Crea una capa suelta en el mapa `a1`, de `(d0, d1)` a `(d2, d3)`, con `flags` (d4) y su `Layer_Info` (a0).
    /// Con el gancho de relleno de serie (0) se borra con la pluma 0; con `LAYERS_NOBACKFILL` (1), no; uno de 68k aún no
    /// se llama (no se borra).
    fn crear_capa<M: Memoria>(&mut self, ex: &mut Exec<M>, r: &Registros, gancho: u32) -> Result<u32, Fallo> {
        let (li, bm) = (r.a[0], r.a[1]);
        let rc = Rect::nuevo(palabra(r.d[0]), palabra(r.d[1]), palabra(r.d[2]), palabra(r.d[3]));
        if bm == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        if rc.vacio() || rc.x0 < 0 || rc.y0 < 0 {
            return Err(Fallo::TamanoMalo { ancho: rc.x1 - rc.x0 + 1, alto: rc.y1 - rc.y0 + 1 });
        }
        // El mapa tiene que ser uno que graphics sepa pintar.
        self.mapa(ex, puntero(bm)?)?;
        let capa = ex.reservar(layer::TAMANO, memf::CLEAR)?;
        let rp = ex.reservar(rp_::TAMANO, memf::CLEAR)?;
        let cr = ex.reservar(cliprect::TAMANO, memf::CLEAR)?;
        let dano = self.region_del_sistema(ex)?;
        self.init_rast_port(ex, rp)?;
        ex.e32(rp + rp_::BITMAP, bm)?;
        ex.e32(rp + rp_::LAYER, capa)?;
        ex.e32(capa + layer::RP, rp)?;
        ex.e32(capa + layer::LAYERINFO, li)?;
        ex.e16(capa + layer::FLAGS, r.d[4] as u16)?;
        ex.e32(capa + layer::SUPERBITMAP, r.a[2])?;
        ex.e32(capa + layer::CLIPRECT, cr)?;
        ex.e32(capa + layer::DAMAGELIST, dano)?;
        for (base, v) in [(capa + layer::BOUNDS, rc), (cr + cliprect::BOUNDS, rc)] {
            ex.e16(base + rectangle::MINX, v.x0 as u16)?;
            ex.e16(base + rectangle::MINY, v.y0 as u16)?;
            ex.e16(base + rectangle::MAXX, v.x1 as u16)?;
            ex.e16(base + rectangle::MAXY, v.y1 as u16)?;
        }
        ex.e16(capa + layer::WIDTH, rc.ancho() as u16)?;
        ex.e16(capa + layer::HEIGHT, rc.alto() as u16)?;
        self.gfx.capas.insert(capa, rc);
        self.gfx.origenes.insert(capa, (rc.x0, rc.y0));
        self.capas.sueltas.insert(capa, Suelta { rp, cliprect: cr, dano, tarea: ex.tarea_actual() });
        if gancho == 0 {
            self.set_rast(ex, rp, 0)?;
        }
        Ok(capa)
    }

    /// Suelta una capa suelta (`DeleteLayer`). Una que no lo es da un fallo y no se toca.
    pub(crate) fn borrar_capa<M: Memoria>(&mut self, ex: &mut Exec<M>, capa: u32) -> Result<(), Fallo> {
        let Some(s) = self.capas.sueltas.remove(&capa) else {
            return Err(Fallo::NoReservado { dir: capa });
        };
        self.gfx.capas.remove(&capa);
        self.gfx.origenes.remove(&capa);
        self.soltar_dano(ex, s.dano);
        ex.soltar(s.cliprect, cliprect::TAMANO)?;
        ex.soltar(s.rp, rp_::TAMANO)?;
        ex.soltar(capa, layer::TAMANO)
    }

    /// Las capas sueltas de una tarea que acaba.
    pub(crate) fn capas_tarea_muerta<M: Memoria>(&mut self, ex: &mut Exec<M>, t: u32) {
        let suyas: Vec<u32> = self.capas.sueltas.iter().filter(|(_, s)| s.tarea == t).map(|(c, _)| *c).collect();
        for c in suyas {
            if let Err(f) = self.borrar_capa(ex, c) {
                self.avisar(ex, "DeleteLayer", f);
            }
        }
    }

    pub(crate) fn llamar_keymap<M: Memoria>(&mut self, ex: &mut Exec<M>, f: usize, r: &mut Registros) -> Result<u32, Fallo> {
        let (d, a) = (r.d, r.a);
        Ok(match f {
            SETKEYMAPDEFAULT => {
                if a[0] != 0 {
                    self.capas.km_defecto = a[0];
                }
                0
            }
            ASKKEYMAPDEFAULT => self.capas.km_defecto,
            MAPRAWKEY => {
                if a[0] == 0 {
                    return Err(Fallo::Exec(caja_exec::Fallo::PunteroNulo));
                }
                if ex.l8(a[0] + input_event::CLASS)? != input_event::IECLASS_RAWKEY {
                    return Ok(0);
                }
                let code = ex.l16(a[0] + input_event::CODE)?;
                let q = ex.l16(a[0] + input_event::QUALIFIER)?;
                if code & 0x80 != 0 {
                    return Ok(0);
                }
                let c = code as u8;
                let mut v = Vec::new();
                if let Some(s) = secuencia(c, q & (ie::LSHIFT | ie::RSHIFT) != 0) {
                    v.push(0x9B);
                    v.extend_from_slice(&s);
                } else if let Some(ch) = caracter(c, q) {
                    v.push(ch);
                }
                let largo = d[1] as i32;
                if v.len() as i32 > largo.max(0) {
                    return Ok(u32::MAX);
                }
                for (i, &b) in v.iter().enumerate() {
                    ex.e8(a[1] + i as u32, b)?;
                }
                v.len() as u32
            }
            MAPANSI => {
                // De cada carácter, la tecla y sus calificadores (dos bytes por carácter).
                let (n, largo) = (d[0].min(4096), d[1] as i32);
                let mut puestos = 0i32;
                for i in 0..n {
                    let ch = ex.l8(a[0] + i)?;
                    let Some((c, q)) = (0u8..0x68)
                        .flat_map(|c| [(c, 0u16), (c, ie::LSHIFT)])
                        .find(|&(c, q)| caracter(c, q) == Some(ch))
                    else {
                        return Ok(0);
                    };
                    if (puestos + 1) * 2 > largo.max(0) {
                        return Ok(u32::MAX);
                    }
                    ex.e8(a[1] + puestos as u32 * 2, c)?;
                    ex.e8(a[1] + puestos as u32 * 2 + 1, q as u8)?;
                    puestos += 1;
                }
                puestos as u32
            }
            _ => return Err(Fallo::NoImplementada { n: self.numeros.keymap + f as u16 }),
        })
    }
}

/// Una `struct KeyMap` con la distribución estadounidense, para `AskKeyMapDefault` (RKRM, "Keymap Library": las
/// teclas bajas, `$00`-`$3F`, y las altas, `$40`-`$67`; cada una con su tipo, cuatro bytes de caracteres y los bits de
/// las que cambian con Bloq Mayús y las que repiten).
fn crear_keymap<M: Memoria>(ex: &mut Exec<M>) -> Result<u32, Fallo> {
    let km = ex.reservar(keymap::TAMANO, memf::PUBLIC | memf::CLEAR)?;
    for (alta, base, n) in [(false, 0u8, 64u32), (true, 0x40u8, 40u32)] {
        let tipos = ex.reservar(n, memf::PUBLIC | memf::CLEAR)?;
        let mapa = ex.reservar(n * 4, memf::PUBLIC | memf::CLEAR)?;
        let capsable = ex.reservar(8, memf::PUBLIC | memf::CLEAR)?;
        let repeatable = ex.reservar(8, memf::PUBLIC | memf::CLEAR)?;
        for i in 0..n {
            let c = base + i as u8;
            let (sin, con) = (caracter(c, 0), caracter(c, ie::LSHIFT));
            let tipo = match (sin, con) {
                (Some(_), Some(y)) if Some(y) != sin => keymap::KCF_SHIFT,
                (Some(_), _) => keymap::KC_NOQUAL,
                _ => keymap::KCF_NOP,
            };
            ex.e8(tipos + i, tipo)?;
            // Los cuatro bytes, del más alto al más bajo: Alt+Mayús, Alt, Mayús, sin nada (con KCF_SHIFT, los dos de
            // abajo).
            let v = (con.unwrap_or(0) as u32) << 8 | sin.unwrap_or(0) as u32;
            ex.e32(mapa + i * 4, v)?;
            if sin.is_some_and(|x| x.is_ascii_lowercase()) {
                let b = ex.l8(capsable + i / 8)?;
                ex.e8(capsable + i / 8, b | 1 << (i % 8))?;
            }
            if sin.is_some() {
                let b = ex.l8(repeatable + i / 8)?;
                ex.e8(repeatable + i / 8, b | 1 << (i % 8))?;
            }
        }
        let (t, m, c, r) = if alta {
            (keymap::HI_TYPES, keymap::HI_MAP, keymap::HI_CAPSABLE, keymap::HI_REPEATABLE)
        } else {
            (keymap::LO_TYPES, keymap::LO_MAP, keymap::LO_CAPSABLE, keymap::LO_REPEATABLE)
        };
        ex.e32(km + t, tipos)?;
        ex.e32(km + m, mapa)?;
        ex.e32(km + c, capsable)?;
        ex.e32(km + r, repeatable)?;
    }
    Ok(km)
}
