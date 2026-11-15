//! El mapa de un programa que es el de una pantalla (`SA_BitMap`, `CUSTOMBITMAP`): saber qué ha cambiado para
//! presentar solo eso.
//!
//! - Lo que escriben graphics, cybergraphics y Picasso96 se apunta al escribirlo (el `Pintor`, `poner_trozo`,
//!   `ExtractColor`, `BltClear` sobre su memoria) y lo que el programa escribe tras `LockBitMap`, al soltarlo
//!   (`UBMI_UPDATERECTS` o el mapa entero), como en las ventanas.
//! - Lo que el programa escribe directamente en sus planos o en sus píxeles, sin cerrojo ni llamadas (lo que haría con
//!   una tarjeta de verdad), no pasa por aquí. Para eso, cada fila tiene una huella de lo último presentado y cada
//!   `SONDEO_US` se miran todas: una fila distinta que nadie apuntó es una escritura directa. Desde entonces el mapa se
//!   mira en cada presentación (solo las filas distintas se presentan), hasta `QUIETOS` sondeos seguidos sin ninguna.

use alloc::vec::Vec;

use caja_exec::{Exec, Memoria};

use crate::graphics::Mapa;
use crate::lienzo::Rect;
use crate::{CajaGrafica, Compositor, Superficie};

/// Cada cuánto se mira si un programa escribe en su mapa sin decirlo (µs).
pub(crate) const SONDEO_US: u64 = 500_000;
/// Sondeos seguidos sin escrituras directas para dejar de mirar en cada presentación.
pub(crate) const QUIETOS: u32 = 20;

/// Lo que se sabe del mapa de un programa que es el de una pantalla.
pub(crate) struct Vigilado {
    /// Lo apuntado desde la última presentación (en coordenadas del mapa).
    pub dano: Rect,
    /// La huella de cada fila tal como se presentó (vacío: aún no se ha presentado).
    pub huellas: Vec<u64>,
    /// La copia en ARGB de lo presentado (lo que lee el compositor).
    pub presentado: Vec<u32>,
    /// Se le ha visto escribir sin llamadas: se mira en cada presentación.
    pub directo: bool,
    /// Sondeos seguidos sin escrituras directas (con `directo`).
    pub quietos: u32,
    /// Cuándo fue el último sondeo (µs).
    pub sondeo: u64,
    /// Cuántas escrituras directas se han visto (para las pruebas y la bitácora).
    pub vistas: u32,
}

impl Vigilado {
    pub(crate) fn nuevo() -> Vigilado {
        Vigilado { dano: Rect::VACIO, huellas: Vec::new(), presentado: Vec::new(), directo: false, quietos: 0, sondeo: 0, vistas: 0 }
    }
}

/// La huella de unos bytes (de 8 en 8; no criptográfica: solo para ver si cambió).
fn huella(h: u64, b: &[u8]) -> u64 {
    let mut h = h;
    let mut trozos = b.chunks_exact(8);
    for t in &mut trozos {
        let w = u64::from_le_bytes([t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7]]);
        h = (h.rotate_left(23) ^ w).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    for &x in trozos.remainder() {
        h = (h.rotate_left(23) ^ x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    h
}

/// La huella de la fila `y` de un mapa planar o chunky (0 si su memoria no se puede leer).
pub(crate) fn huella_fila<M: Memoria>(mem: &M, mapa: &Mapa, y: u32) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64 ^ y as u64;
    match mapa {
        Mapa::Planar(p) => {
            for &pl in p.planos.iter().take(p.prof as usize) {
                if pl == 0 || pl == crate::dibujo::PLANO_UNOS {
                    continue;
                }
                let d = pl.wrapping_add(y.wrapping_mul(p.bpr));
                if let Ok(b) = mem.leer(d, p.ancho / 8) {
                    h = huella(h, b);
                }
            }
        }
        Mapa::Chunky(c) => {
            if let Ok(b) = mem.leer(c.base.wrapping_add(y.wrapping_mul(c.bpr)), c.ancho * c.fmt.bpp()) {
                h = huella(h, b);
            }
        }
        Mapa::Nativo => {}
    }
    h
}

/// Las filas de un mapa que tocan `[dir, dir + largo)`.
fn filas_en(mapa: &Mapa, dir: u32, largo: u32) -> Option<(u32, u32)> {
    let fin = dir as u64 + largo as u64;
    let mut r: Option<(u32, u32)> = None;
    let mut tramo = |base: u32, bpr: u32, filas: u32| {
        if bpr == 0 || filas == 0 {
            return;
        }
        let tope = base as u64 + bpr as u64 * filas as u64;
        let (a, b) = ((dir as u64).max(base as u64), fin.min(tope));
        if a >= b {
            return;
        }
        let (y0, y1) = (((a - base as u64) / bpr as u64) as u32, ((b - 1 - base as u64) / bpr as u64) as u32);
        r = Some(r.map_or((y0, y1), |(p, q)| (p.min(y0), q.max(y1))));
    };
    match mapa {
        Mapa::Planar(p) => {
            for &pl in p.planos.iter().take(p.prof as usize) {
                if pl != 0 && pl != crate::dibujo::PLANO_UNOS {
                    tramo(pl, p.bpr, p.filas);
                }
            }
        }
        Mapa::Chunky(c) => tramo(c.base, c.bpr, c.alto),
        Mapa::Nativo => {}
    }
    r
}

/// Los µs de una hora de intuition.
fn us((s, m): (u32, u32)) -> u64 {
    s as u64 * 1_000_000 + m as u64
}

impl<C: Compositor> CajaGrafica<C> {
    /// Apunta como cambiado `r` (en coordenadas del mapa) de `bm`: en un lienzo, para el compositor; en el mapa de un
    /// programa que es el de una pantalla, para presentarlo.
    pub(crate) fn tocar(&mut self, bm: u32, r: Rect) {
        if let Some(l) = self.gfx.lienzos.get_mut(&bm) {
            l.danar(r);
        } else if let Some(v) = self.gfx.vigilados.get_mut(&bm) {
            v.dano = v.dano.union(&r);
        }
    }

    /// Lo escrito en `[dir, dir + largo)` de la memoria (`BltClear`): las filas que toca de los mapas vigilados.
    pub(crate) fn tocar_memoria<M: Memoria>(&mut self, ex: &Exec<M>, dir: u32, largo: u32) {
        let bms: Vec<u32> = self.gfx.vigilados.keys().copied().collect();
        for bm in bms {
            let Ok(mapa) = self.mapa(ex, bm) else { continue };
            if let Some((y0, y1)) = filas_en(&mapa, dir, largo) {
                let an = match mapa {
                    Mapa::Planar(p) => p.ancho,
                    Mapa::Chunky(c) => c.ancho,
                    Mapa::Nativo => 0,
                };
                self.tocar(bm, Rect::nuevo(0, y0 as i32, an as i32 - 1, y1 as i32));
            }
        }
    }

    /// El compositor perdió el búfer de la pantalla `s`: se presenta entera la próxima vez.
    pub(crate) fn exponer_pantalla<M: Memoria>(&mut self, ex: &mut Exec<M>, s: u32) {
        let Some(programa) = self.intui.pantallas.get(&s).map(|p| p.mapa_programa) else { return };
        let bm = if programa != 0 { programa } else { s + crate::estructuras::screen::BITMAP };
        if let Some(v) = self.gfx.vigilados.get_mut(&bm) {
            v.huellas.clear();
            v.presentado.clear();
        } else if let Some(mut l) = crate::graphics::vista(&mut self.gfx.lienzos, ex, bm) {
            use crate::lienzo::Pinta;
            let r = l.rect();
            l.danar(r);
        }
    }

    /// Cuántas escrituras directas (sin llamadas) se han visto en el mapa del programa de una pantalla, y si ahora se
    /// mira en cada presentación (para las pruebas).
    pub fn escrituras_directas(&self, s: u32) -> Option<(u32, bool)> {
        let p = self.intui.pantallas.get(&s)?;
        self.gfx.vigilados.get(&p.mapa_programa).map(|v| (v.vistas, v.directo))
    }

    /// Presenta lo que ha cambiado en el mapa de un programa que es el de la pantalla `s` (de `an` x `al`, con la
    /// paleta `cm`, en la superficie `sup`).
    pub(crate) fn presentar_mapa_programa<M: Memoria>(&mut self, ex: &Exec<M>, bm: u32, sup: Superficie, an: u32, al: u32, cm: u32) {
        let Ok(mapa) = self.mapa(ex, bm) else { return };
        if let Mapa::Nativo = mapa {
            // Un lienzo (amigo de una ventana): ya lleva lo cambiado.
            let Some(l) = self.gfx.lienzos.get_mut(&bm) else { return };
            let (lan, lal) = (l.ancho(), l.alto());
            if let Some((d, pix, textos)) = l.presentar(&ex.mem) {
                self.comp.presentar(sup, pix, lan, lal, d, textos);
            }
            return;
        }
        let Some(mut v) = self.gfx.vigilados.remove(&bm) else { return };
        let todo = Rect::de_tamano(an, al).interseccion(&match mapa {
            Mapa::Planar(p) => p.rect(),
            Mapa::Chunky(c) => c.rect(),
            Mapa::Nativo => Rect::VACIO,
        });
        let ahora = us(self.intui.hora);
        let primera = v.huellas.is_empty();
        let apuntado = v.dano.interseccion(&todo);
        v.dano = Rect::VACIO;
        let mut dano = if primera { todo } else { apuntado };
        let sondear = primera || v.directo || ahora.wrapping_sub(v.sondeo) >= SONDEO_US;
        if sondear && !todo.vacio() {
            v.sondeo = ahora;
            let mut nuevas = Vec::new();
            if nuevas.try_reserve_exact(todo.alto() as usize).is_err() {
                self.gfx.vigilados.insert(bm, v);
                return;
            }
            let mut ajenas = false;
            for y in 0..todo.alto() {
                let h = huella_fila(&ex.mem, &mapa, y);
                nuevas.push(h);
                if !primera && v.huellas.get(y as usize) != Some(&h) {
                    dano = dano.union(&Rect::nuevo(0, y as i32, todo.x1, y as i32));
                    if (y as i32) < apuntado.y0 || (y as i32) > apuntado.y1 {
                        ajenas = true;
                    }
                }
            }
            v.huellas = nuevas;
            if ajenas {
                v.vistas = v.vistas.saturating_add(1);
                v.directo = true;
                v.quietos = 0;
            } else if v.directo && !primera {
                v.quietos += 1;
                if v.quietos >= QUIETOS {
                    v.directo = false;
                }
            }
        } else if !dano.vacio() {
            for y in dano.y0..=dano.y1 {
                if let Some(h) = v.huellas.get_mut(y as usize) {
                    *h = huella_fila(&ex.mem, &mapa, y as u32);
                }
            }
        }
        if dano.vacio() {
            self.gfx.vigilados.insert(bm, v);
            return;
        }
        let largo = an as usize * al as usize;
        if v.presentado.len() != largo {
            v.presentado.clear();
            if v.presentado.try_reserve_exact(largo).is_err() {
                self.gfx.vigilados.insert(bm, v);
                return;
            }
            v.presentado.resize(largo, 0xFF00_0000);
            dano = todo;
        }
        let de_color = self.de_color(bm, mapa);
        let mut fila = Vec::new();
        for y in dano.y0..=dano.y1 {
            let o = y as usize * an as usize;
            match mapa {
                Mapa::Chunky(c) => {
                    fila.clear();
                    c.leer_fila(&ex.mem, dano.x0, y, dano.ancho(), &mut fila);
                    for (k, &px) in fila.iter().enumerate() {
                        let c = if de_color { px } else { self.color(cm, px as u8) };
                        v.presentado[o + dano.x0 as usize + k] = c | 0xFF00_0000;
                    }
                }
                Mapa::Planar(p) => {
                    for x in dano.x0..=dano.x1 {
                        v.presentado[o + x as usize] = self.color(cm, p.leer(&ex.mem, x, y)) | 0xFF00_0000;
                    }
                }
                Mapa::Nativo => {}
            }
        }
        self.comp.presentar(sup, &v.presentado, an, al, dano, &[]);
        self.gfx.vigilados.insert(bm, v);
    }
}
