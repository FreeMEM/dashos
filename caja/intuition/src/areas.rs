//! El relleno de áreas de graphics.library: `InitArea`, `InitTmpRas`, `AreaMove`, `AreaDraw`, `AreaEllipse` y
//! `AreaEnd` (autodocs de graphics.library; `graphics/rastport.h` para `struct AreaInfo` y `struct TmpRas`).
//!
//! Los vértices van a la tabla del programa (la de `InitArea`, cinco bytes por vértice: cuatro de coordenadas y uno de
//! marca), como en el Amiga, y `AreaEnd` los rellena todos a la vez con la regla par-impar (lo que hace el blitter con
//! su relleno por O exclusivo): cada figura da tramos por fila y los tramos de todas se combinan por O exclusivo. Con
//! las plumas, el modo y el patrón de área del `RastPort`; con `AREAOUTLINE` (`SetOutlinePen`), el borde de los
//! polígonos con la pluma de contorno. La memoria de `InitTmpRas` no hace falta (no hay blitter): solo se apunta.
//!
//! Lo hostil: sin `AreaInfo`, una tabla fuera de la memoria o una cuenta mayor que el tope son un fallo (-1); una
//! elipse con un radio de 0 o negativo no se pinta; las coordenadas enormes se recortan al mapa.

use alloc::vec::Vec;

use caja_exec::{Exec, Memoria};

use crate::acceso::{Acceso, puntero};
use crate::dibujo::Tinta;
use crate::estructuras::{areainfo as ai, rastport as rp_, tmpras};
use crate::lienzo::Rect;
use crate::{CajaGrafica, Compositor, Fallo};

/// Las marcas de la tabla (solo las lee graphics).
const MOVER: u8 = 1;
const DIBUJAR: u8 = 0;
const ELIPSE: u8 = 2;
/// La segunda entrada de una elipse (sus radios).
const RADIOS: u8 = 3;

/// Una `AreaInfo` leída.
struct Info {
    dir: u32,
    vectores: u32,
    marcas: u32,
    cuenta: u32,
    tope: u32,
}

/// Una figura de la tabla.
enum Figura {
    Poligono(Vec<(i32, i32)>),
    Elipse { cx: i32, cy: i32, a: i32, b: i32 },
}

/// Los tramos (cerrados, `[x0, x1]`) de la fila `y` de un polígono, con la regla par-impar: los cortes de sus lados con
/// la fila, por parejas. Cada lado cubre sus filas de arriba sin la de abajo (un vértice cuenta una vez), salvo en la
/// última fila del polígono, que también se rellena (como el borde que dibuja el Amiga antes de rellenar).
fn tramos_poligono(p: &[(i32, i32)], y: i32, sal: &mut Vec<(i32, i32)>) {
    let mut cortes: Vec<i32> = Vec::new();
    let n = p.len();
    let ultima = p.iter().map(|v| v.1).max().unwrap_or(y) == y;
    for k in 0..n {
        let (a, b) = (p[k], p[(k + 1) % n]);
        if a.1 == b.1 {
            continue;
        }
        let (lo, hi) = if a.1 < b.1 { (a, b) } else { (b, a) };
        let dentro = if ultima { y > lo.1 && y <= hi.1 } else { y >= lo.1 && y < hi.1 };
        if !dentro {
            continue;
        }
        let num = (y - lo.1) as i64 * (hi.0 - lo.0) as i64;
        let den = (hi.1 - lo.1) as i64;
        cortes.push(lo.0 + num.div_euclid(den) as i32);
    }
    cortes.sort_unstable();
    for par in cortes.chunks_exact(2) {
        sal.push((par[0], par[1]));
    }
}

/// El tramo de la fila `y` de una elipse, si la corta.
fn tramo_elipse(cx: i32, cy: i32, a: i32, b: i32, y: i32) -> Option<(i32, i32)> {
    let dy = (y - cy).unsigned_abs() as u64;
    let (a, b) = (a as u64, b as u64);
    if dy > b {
        return None;
    }
    // El mayor dx con (dx/a)² + (dy/b)² <= 1.
    let dx = ((a * a * (b * b - dy * dy)) / (b * b)).isqrt() as i32;
    Some((cx - dx, cx + dx))
}

impl<C: Compositor> CajaGrafica<C> {
    /// La `AreaInfo` de un `RastPort`, comprobada.
    fn area_info<M: Memoria>(&self, ex: &Exec<M>, rp: u32) -> Result<Info, Fallo> {
        if rp == 0 {
            return Err(Fallo::RastPortMalo { dir: 0 });
        }
        let dir = ex.l32(puntero(rp)? + rp_::AREAINFO)?;
        if dir == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let dir = puntero(dir)?;
        ex.tramo(dir, ai::TAMANO)?;
        let i = Info {
            dir,
            vectores: ex.l32(dir + ai::VCTRTBL)?,
            marcas: ex.l32(dir + ai::FLAGTBL)?,
            cuenta: ex.l16(dir + ai::COUNT)? as i16 as i32 as u32,
            tope: ex.l16(dir + ai::MAXCOUNT)? as i16 as i32 as u32,
        };
        if i.cuenta > i.tope || i.tope > 0x7FFF {
            return Err(Fallo::Sospechoso { dir });
        }
        ex.tramo(i.vectores, i.tope * 4)?;
        ex.tramo(i.marcas, i.tope)?;
        Ok(i)
    }

    /// `InitArea(areainfo, buffer, maxvectors)`: la tabla de coordenadas al principio del búfer y las marcas detrás.
    pub(crate) fn init_area<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        info: u32,
        buf: u32,
        tope: u32,
    ) -> Result<(), Fallo> {
        let tope = (tope as u16 as i16).max(0) as u32;
        let info = puntero(info)?;
        ex.tramo(info, ai::TAMANO)?;
        if info == 0 || buf == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        ex.tramo(buf, tope * 5)?;
        let marcas = buf + tope * 4;
        ex.e32(info + ai::VCTRTBL, buf)?;
        ex.e32(info + ai::VCTRPTR, buf)?;
        ex.e32(info + ai::FLAGTBL, marcas)?;
        ex.e32(info + ai::FLAGPTR, marcas)?;
        ex.e16(info + ai::COUNT, 0)?;
        ex.e16(info + ai::MAXCOUNT, tope as u16)?;
        ex.e16(info + ai::FIRSTX, 0)?;
        ex.e16(info + ai::FIRSTY, 0)
    }

    /// `InitTmpRas(tmpras, buffer, size)`: solo se apunta (no hay blitter que la necesite). Devuelve `tmpras`.
    pub(crate) fn init_tmp_ras<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        t: u32,
        buf: u32,
        largo: u32,
    ) -> Result<u32, Fallo> {
        if t == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let t = puntero(t)?;
        ex.e32(t + tmpras::RASPTR, buf)?;
        ex.e32(t + tmpras::SIZE, largo)?;
        Ok(t)
    }

    /// Añade un vértice a la tabla. -1 si no cabe.
    fn meter<M: Memoria>(&mut self, ex: &mut Exec<M>, i: &mut Info, x: i32, y: i32, marca: u8) -> Result<bool, Fallo> {
        if i.cuenta >= i.tope {
            return Ok(false);
        }
        let v = i.vectores + i.cuenta * 4;
        ex.e16(v, x as u16)?;
        ex.e16(v + 2, y as u16)?;
        ex.e8(i.marcas + i.cuenta, marca)?;
        i.cuenta += 1;
        ex.e16(i.dir + ai::COUNT, i.cuenta as u16)?;
        ex.e32(i.dir + ai::VCTRPTR, i.vectores + i.cuenta * 4)?;
        ex.e32(i.dir + ai::FLAGPTR, i.marcas + i.cuenta)?;
        Ok(true)
    }

    /// `AreaMove(rp, x, y)`: empieza un polígono (el de antes queda cerrado: se cierra al rellenar). 0, o -1.
    pub(crate) fn area_move<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, x: i32, y: i32) -> Result<u32, Fallo> {
        let mut i = self.area_info(ex, rp)?;
        if !self.meter(ex, &mut i, x, y, MOVER)? {
            return Ok(u32::MAX);
        }
        ex.e16(i.dir + ai::FIRSTX, x as u16)?;
        ex.e16(i.dir + ai::FIRSTY, y as u16)?;
        self.mover_cp(ex, rp, x, y)?;
        Ok(0)
    }

    /// `AreaDraw(rp, x, y)`: un vértice más del polígono. 0, o -1.
    pub(crate) fn area_draw<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, x: i32, y: i32) -> Result<u32, Fallo> {
        let mut i = self.area_info(ex, rp)?;
        if !self.meter(ex, &mut i, x, y, DIBUJAR)? {
            return Ok(u32::MAX);
        }
        self.mover_cp(ex, rp, x, y)?;
        Ok(0)
    }

    /// `AreaEllipse(rp, cx, cy, a, b)`: una elipse (dos vértices: el centro y los radios). 0, o -1.
    pub(crate) fn area_ellipse<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        rp: u32,
        cx: i32,
        cy: i32,
        a: i32,
        b: i32,
    ) -> Result<u32, Fallo> {
        let mut i = self.area_info(ex, rp)?;
        if i.tope - i.cuenta < 2 {
            return Ok(u32::MAX);
        }
        self.meter(ex, &mut i, cx, cy, ELIPSE)?;
        self.meter(ex, &mut i, a, b, RADIOS)?;
        Ok(0)
    }

    fn mover_cp<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32, x: i32, y: i32) -> Result<(), Fallo> {
        ex.e16(rp + rp_::CP_X, x as u16)?;
        ex.e16(rp + rp_::CP_Y, y as u16)
    }

    /// `AreaEnd(rp)`: rellena todo lo de la tabla y la deja vacía. 0, o -1.
    pub(crate) fn area_end<M: Memoria>(&mut self, ex: &mut Exec<M>, rp: u32) -> Result<u32, Fallo> {
        let i = self.area_info(ex, rp)?;
        // Las figuras de la tabla.
        let mut figuras: Vec<Figura> = Vec::new();
        let mut k = 0;
        while k < i.cuenta {
            let v = i.vectores + k * 4;
            let (x, y) = (ex.l16(v)? as i16 as i32, ex.l16(v + 2)? as i16 as i32);
            match ex.l8(i.marcas + k)? {
                ELIPSE if k + 1 < i.cuenta => {
                    let (a, b) = (ex.l16(v + 4)? as i16 as i32, ex.l16(v + 6)? as i16 as i32);
                    if a > 0 && b > 0 {
                        figuras.push(Figura::Elipse { cx: x, cy: y, a, b });
                    }
                    k += 2;
                    continue;
                }
                MOVER => figuras.push(Figura::Poligono(alloc::vec![(x, y)])),
                _ => match figuras.last_mut() {
                    Some(Figura::Poligono(p)) => p.push((x, y)),
                    // Un AreaDraw sin AreaMove: empieza en el (0, 0) de la tabla vacía, como uno nuevo.
                    _ => figuras.push(Figura::Poligono(alloc::vec![(x, y)])),
                },
            }
            k += 1;
        }
        // La tabla, vacía para lo siguiente (aunque falle el dibujo).
        ex.e16(i.dir + ai::COUNT, 0)?;
        ex.e32(i.dir + ai::VCTRPTR, i.vectores)?;
        ex.e32(i.dir + ai::FLAGPTR, i.marcas)?;
        let info = self.leer_rp(ex, rp)?;
        let contorno = info.flags & rp_::AREAOUTLINE != 0;
        let aol = ex.l8(rp + rp_::AOLPEN)?;
        let patron = self.patron_area(ex, &info)?;
        let cm = self.cm_de(info.bm);
        let tinta_contorno = Tinta { pen: aol, argb: self.color(cm, aol) };
        let mut p = self.pintor(ex, &info)?;
        // Las filas que pueden tener algo: las de las figuras, dentro del recorte.
        let mut caja = Rect::nuevo(0, 0, -1, -1);
        for f in &figuras {
            let r = match f {
                Figura::Poligono(v) => v.iter().fold(Rect::nuevo(v[0].0, v[0].1, v[0].0, v[0].1), |r, &(x, y)| {
                    Rect::nuevo(r.x0.min(x), r.y0.min(y), r.x1.max(x), r.y1.max(y))
                }),
                Figura::Elipse { cx, cy, a, b } => Rect::nuevo(cx - a, cy - b, cx + a, cy + b),
            };
            caja = caja.union(&r);
        }
        let caja = caja.interseccion(&p.clip);
        let mut tramos: Vec<(i32, i32)> = Vec::new();
        let mut bordes: Vec<i32> = Vec::new();
        for y in caja.y0..=caja.y1 {
            tramos.clear();
            for f in &figuras {
                match f {
                    Figura::Poligono(v) => tramos_poligono(v, y, &mut tramos),
                    Figura::Elipse { cx, cy, a, b } => tramos.extend(tramo_elipse(*cx, *cy, *a, *b, y)),
                }
            }
            // O exclusivo de todos los tramos: los bordes (semiabiertos) en orden, por parejas.
            bordes.clear();
            for &(x0, x1) in &tramos {
                bordes.push(x0);
                bordes.push(x1 + 1);
            }
            bordes.sort_unstable();
            let fila = if patron.is_empty() { 0xFFFF } else { patron[y.rem_euclid(patron.len() as i32) as usize] };
            for par in bordes.chunks_exact(2) {
                let (x0, x1) = (par[0].max(caja.x0), (par[1] - 1).min(caja.x1));
                for x in x0..=x1 {
                    p.punto(x, y, fila & (0x8000 >> (x & 15)) != 0);
                }
            }
        }
        if contorno {
            p.a = tinta_contorno;
            let mut cnt = 0u8;
            for f in &figuras {
                if let Figura::Poligono(v) = f {
                    for k in 0..v.len() {
                        let (a, b) = (v[k], v[(k + 1) % v.len()]);
                        p.linea(a.0, a.1, b.0, b.1, 0xFFFF, &mut cnt, false);
                    }
                }
            }
        }
        Ok(0)
    }
}
