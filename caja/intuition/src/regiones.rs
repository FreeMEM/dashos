//! Las regiones de graphics.library: `NewRegion`, `DisposeRegion`, `ClearRegion`, `OrRectRegion`, `AndRectRegion`,
//! `ClearRectRegion`, `XorRectRegion`, `OrRegionRegion`, `AndRegionRegion` y `XorRegionRegion` (autodocs de
//! graphics.library; `graphics/regions.h` del NDK para la disposición).
//!
//! Una `struct Region` vive en la memoria del 68k porque los programas la recorren a mano (MUI mira la lista de una
//! capa para saber qué repintar): sus límites (`bounds`) y una lista doble de `struct RegionRectangle`, cuyos límites
//! son relativos a la esquina de arriba a la izquierda de los de la región. Aquí se lee entera, se opera en Rust y se
//! escribe de nuevo, siempre en forma de bandas (de arriba abajo; en cada banda, de izquierda a derecha, sin solaparse
//! ni tocarse), que es la que dejan las funciones del Amiga.
//!
//! Lo hostil: una lista en círculo o de más de `MAX_RECTANGULOS`, punteros fuera de la memoria o una región nula son un
//! aviso y `FALSE`, sin tocar nada; un rectángulo con el máximo por debajo del mínimo es vacío. Si no hay memoria para
//! el resultado, la región se queda como estaba (`FALSE`). Solo se sueltan los nodos que reservó graphics.

use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Memoria};

use crate::acceso::{Acceso, puntero};
use crate::estructuras::{rectangle, region, region_rectangle as rr};
use crate::lienzo::Rect;
use crate::{CajaGrafica, Compositor, Fallo, Tabla};

/// Tope de rectángulos de una región (al leerla y al escribir un resultado).
pub const MAX_RECTANGULOS: usize = 16384;

/// Qué hacer con dos regiones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Unión (`Or`).
    O,
    /// Intersección (`And`).
    Y,
    /// Diferencia simétrica (`Xor`).
    Xor,
    /// Lo de la primera que no está en la segunda (`ClearRectRegion`).
    Menos,
}

impl Op {
    fn dentro(self, a: bool, b: bool) -> bool {
        match self {
            Op::O => a || b,
            Op::Y => a && b,
            Op::Xor => a != b,
            Op::Menos => a && !b,
        }
    }
}

/// Los tramos en x (semiabiertos, `[x0, x1)`) de los rectángulos activos, en orden y fundidos.
fn tramos(activos: &[Rect]) -> Vec<(i32, i32)> {
    let mut t: Vec<(i32, i32)> = activos.iter().map(|r| (r.x0, r.x1 + 1)).collect();
    t.sort_unstable();
    let mut f: Vec<(i32, i32)> = Vec::with_capacity(t.len());
    for (a, b) in t {
        match f.last_mut() {
            Some(u) if a <= u.1 => u.1 = u.1.max(b),
            _ => f.push((a, b)),
        }
    }
    f
}

/// Combina dos listas de tramos ordenadas y fundidas.
fn combinar(a: &[(i32, i32)], b: &[(i32, i32)], op: Op) -> Vec<(i32, i32)> {
    let mut xs: Vec<i32> = a.iter().chain(b).flat_map(|&(x0, x1)| [x0, x1]).collect();
    xs.sort_unstable();
    xs.dedup();
    let (mut i, mut j) = (0, 0);
    let mut f: Vec<(i32, i32)> = Vec::new();
    for v in xs.windows(2) {
        let x = v[0];
        while i < a.len() && a[i].1 <= x {
            i += 1;
        }
        while j < b.len() && b[j].1 <= x {
            j += 1;
        }
        let (en_a, en_b) = (i < a.len() && a[i].0 <= x, j < b.len() && b[j].0 <= x);
        if op.dentro(en_a, en_b) {
            match f.last_mut() {
                Some(u) if u.1 == x => u.1 = v[1],
                _ => f.push((x, v[1])),
            }
        }
    }
    f
}

/// Los rectángulos de una lista que cubren cada banda, de arriba abajo: los que empiezan en ella entran y los que ya
/// acabaron salen (así una región larga en bandas no cuesta el cuadrado de su largo).
struct Barrido {
    rs: Vec<Rect>,
    sig: usize,
    activos: Vec<Rect>,
}

impl Barrido {
    fn nuevo(rs: &[Rect]) -> Barrido {
        let mut rs: Vec<Rect> = rs.iter().filter(|r| !r.vacio()).copied().collect();
        rs.sort_unstable_by_key(|r| r.y0);
        Barrido { rs, sig: 0, activos: Vec::new() }
    }
    /// Los tramos de la banda que empieza en `y` (las bandas se recorren en orden y sus bordes son todos los de los
    /// rectángulos, así que uno activo la cubre entera).
    fn banda(&mut self, y: i32) -> Vec<(i32, i32)> {
        self.activos.retain(|r| r.y1 >= y);
        while self.sig < self.rs.len() && self.rs[self.sig].y0 <= y {
            if self.rs[self.sig].y1 >= y {
                self.activos.push(self.rs[self.sig]);
            }
            self.sig += 1;
        }
        tramos(&self.activos)
    }
}

/// `a op b`, en forma de bandas: rectángulos que no se solapan, de arriba abajo y de izquierda a derecha, con las bandas
/// iguales y seguidas ya juntas. `None` si el resultado pasa de `MAX_RECTANGULOS`.
pub fn operar(a: &[Rect], b: &[Rect], op: Op) -> Option<Vec<Rect>> {
    let mut ys: Vec<i32> = a.iter().chain(b).filter(|r| !r.vacio()).flat_map(|r| [r.y0, r.y1 + 1]).collect();
    ys.sort_unstable();
    ys.dedup();
    let (mut ba, mut bb) = (Barrido::nuevo(a), Barrido::nuevo(b));
    let mut sal: Vec<Rect> = Vec::new();
    // Los rectángulos de la banda de antes (desde dónde en `sal`), sus tramos y dónde acaba, para alargarlos si la de
    // ahora es igual y va pegada.
    let mut antes: (usize, Vec<(i32, i32)>, i32) = (0, Vec::new(), i32::MIN);
    for v in ys.windows(2) {
        let (y0, y1) = (v[0], v[1]);
        let t = combinar(&ba.banda(y0), &bb.banda(y0), op);
        if t.is_empty() {
            antes = (sal.len(), Vec::new(), i32::MIN);
            continue;
        }
        if antes.2 == y0 && antes.1 == t {
            for r in &mut sal[antes.0..] {
                r.y1 = y1 - 1;
            }
        } else {
            let desde = sal.len();
            sal.extend(t.iter().map(|&(x0, x1)| Rect::nuevo(x0, y0, x1 - 1, y1 - 1)));
            if sal.len() > MAX_RECTANGULOS {
                return None;
            }
            antes = (desde, t, y0);
        }
        antes.2 = y1;
    }
    Some(sal)
}

/// La caja que encierra a todos.
pub fn limites(rs: &[Rect]) -> Rect {
    rs.iter().fold(Rect::nuevo(0, 0, -1, -1), |u, r| u.union(r))
}

/// Lo que graphics lleva de las regiones: los nodos y las regiones que reservó, con su dueño al reservarlos (solo esos
/// se sueltan: un nodo que puso el programa a mano o uno ya soltado y vuelto a reservar por otro no).
#[derive(Default)]
pub(crate) struct Estado {
    pub nodos: Tabla<u32>,
    pub regiones: Tabla<u32>,
}

fn i16_de(v: u16) -> i32 {
    v as i16 as i32
}

impl<C: Compositor> CajaGrafica<C> {
    /// Si `d` es un bloque de `largo` que reservó graphics y sigue siendo de quien era.
    fn es_nuestro<M: Memoria>(ex: &Exec<M>, tabla: &Tabla<u32>, d: u32, largo: u32) -> bool {
        match (tabla.get(&d), ex.bloque(d)) {
            // exec redondea los bloques a 8 bytes.
            (Some(&dueno), Some(b)) => b.largo == (largo + 7) & !7 && b.dueno == dueno,
            _ => false,
        }
    }

    /// Lee un `struct Rectangle`.
    pub(crate) fn leer_rectangulo<M: Memoria>(&self, ex: &Exec<M>, d: u32) -> Result<Rect, Fallo> {
        if d == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let d = puntero(d)?;
        Ok(Rect::nuevo(
            i16_de(ex.l16(d + rectangle::MINX)?),
            i16_de(ex.l16(d + rectangle::MINY)?),
            i16_de(ex.l16(d + rectangle::MAXX)?),
            i16_de(ex.l16(d + rectangle::MAXY)?),
        ))
    }

    /// Lee una región: sus rectángulos en coordenadas absolutas y sus nodos. Una lista en círculo, demasiado larga o
    /// fuera de la memoria es un fallo.
    pub(crate) fn leer_region<M: Memoria>(&self, ex: &Exec<M>, reg: u32) -> Result<(Vec<Rect>, Vec<u32>), Fallo> {
        if reg == 0 {
            return Err(caja_exec::Fallo::PunteroNulo.into());
        }
        let reg = puntero(reg)?;
        ex.tramo(reg, region::TAMANO)?;
        let b = self.leer_rectangulo(ex, reg + region::BOUNDS)?;
        let (bx, by) = (b.x0 as i16, b.y0 as i16);
        let mut rs = Vec::new();
        let mut nodos = Vec::new();
        let mut n = ex.l32(reg + region::REGIONRECTANGLE)?;
        while n != 0 {
            if nodos.len() >= MAX_RECTANGULOS {
                return Err(Fallo::Sospechoso { dir: reg });
            }
            let n_ = puntero(n)?;
            ex.tramo(n_, rr::TAMANO)?;
            let r = self.leer_rectangulo(ex, n_ + rr::BOUNDS)?;
            // Relativos a los límites, en aritmética de WORD (como el 68k).
            let abs = |o: i16, v: i32| o.wrapping_add(v as i16) as i32;
            rs.push(Rect::nuevo(abs(bx, r.x0), abs(by, r.y0), abs(bx, r.x1), abs(by, r.y1)));
            nodos.push(n_);
            n = ex.l32(n_ + rr::NEXT)?;
        }
        // Un nodo repetido es un círculo (o dos nodos que se pisan): no se sigue.
        let mut vistos = nodos.clone();
        vistos.sort_unstable();
        if vistos.windows(2).any(|v| v[0] == v[1]) {
            return Err(Fallo::Sospechoso { dir: reg });
        }
        Ok((rs, nodos))
    }

    /// Escribe `rs` (ya en bandas) en la región (con `sistema`, sus nodos son del sistema: los de las capas de las
    /// ventanas): primero reserva los nodos nuevos (si no hay memoria, la región se queda
    /// como estaba y da `Ok(false)`), después suelta los viejos que son de graphics y enlaza los nuevos.
    pub(crate) fn escribir_region<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        reg: u32,
        viejos: &[u32],
        rs: &[Rect],
        sistema: bool,
    ) -> Result<bool, Fallo> {
        let mut nuevos = Vec::with_capacity(rs.len());
        for _ in rs {
            let d = ex.alloc_mem(rr::TAMANO, memf::PUBLIC | memf::CLEAR)?;
            if d == 0 {
                for &n in &nuevos {
                    ex.free_mem(n, rr::TAMANO)?;
                    self.reg.nodos.remove(&n);
                }
                return Ok(false);
            }
            if sistema {
                ex.cambiar_dueno(d, 0);
            }
            let dueno = ex.bloque(d).map(|b| b.dueno).unwrap_or(0);
            self.reg.nodos.insert(d, dueno);
            nuevos.push(d);
        }
        for &n in viejos {
            if Self::es_nuestro(ex, &self.reg.nodos, n, rr::TAMANO) {
                ex.free_mem(n, rr::TAMANO)?;
            }
            self.reg.nodos.remove(&n);
        }
        let b = limites(rs);
        let b = if b.vacio() { Rect::nuevo(0, 0, 0, 0) } else { b };
        let poner = |ex: &mut Exec<M>, d: u32, r: Rect| -> Result<(), Fallo> {
            ex.e16(d + rectangle::MINX, r.x0 as u16)?;
            ex.e16(d + rectangle::MINY, r.y0 as u16)?;
            ex.e16(d + rectangle::MAXX, r.x1 as u16)?;
            ex.e16(d + rectangle::MAXY, r.y1 as u16)
        };
        poner(ex, reg + region::BOUNDS, b)?;
        let rel = |v: i32, o: i32| (v as i16).wrapping_sub(o as i16) as i32;
        for (i, (&n, r)) in nuevos.iter().zip(rs).enumerate() {
            ex.e32(n + rr::NEXT, nuevos.get(i + 1).copied().unwrap_or(0))?;
            ex.e32(n + rr::PREV, if i == 0 { 0 } else { nuevos[i - 1] })?;
            poner(ex, n + rr::BOUNDS, Rect::nuevo(rel(r.x0, b.x0), rel(r.y0, b.y0), rel(r.x1, b.x0), rel(r.y1, b.y0)))?;
        }
        ex.e32(reg + region::REGIONRECTANGLE, nuevos.first().copied().unwrap_or(0))?;
        Ok(true)
    }

    /// `region op rs`, escrito en `region`.
    fn operar_region<M: Memoria>(&mut self, ex: &mut Exec<M>, reg: u32, rs: &[Rect], op: Op) -> Result<u32, Fallo> {
        let (actual, nodos) = self.leer_region(ex, reg)?;
        let Some(res) = operar(&actual, rs, op) else { return Ok(0) };
        Ok(self.escribir_region(ex, reg, &nodos, &res, false)? as u32)
    }

    /// `NewRegion()`: una región vacía, o 0.
    pub(crate) fn new_region<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<u32, Fallo> {
        let d = ex.alloc_mem(region::TAMANO, memf::PUBLIC | memf::CLEAR)?;
        if d != 0 {
            let dueno = ex.bloque(d).map(|b| b.dueno).unwrap_or(0);
            self.reg.regiones.insert(d, dueno);
        }
        Ok(d)
    }

    /// `ClearRegion(region)`: la deja vacía.
    pub(crate) fn clear_region<M: Memoria>(&mut self, ex: &mut Exec<M>, reg: u32) -> Result<(), Fallo> {
        let (_, nodos) = self.leer_region(ex, reg)?;
        self.escribir_region(ex, reg, &nodos, &[], false)?;
        Ok(())
    }

    /// `DisposeRegion(region)`: la vacía y la suelta (0 no hace nada). Una que no salió de `NewRegion` se vacía y se
    /// avisa, sin soltarla.
    pub(crate) fn dispose_region<M: Memoria>(&mut self, ex: &mut Exec<M>, reg: u32) -> Result<(), Fallo> {
        if reg == 0 {
            return Ok(());
        }
        self.clear_region(ex, reg)?;
        if Self::es_nuestro(ex, &self.reg.regiones, reg, region::TAMANO) {
            self.reg.regiones.remove(&reg);
            ex.free_mem(reg, region::TAMANO)?;
            Ok(())
        } else {
            Err(Fallo::NoReservado { dir: reg })
        }
    }

    /// `OrRectRegion`, `AndRectRegion`, `ClearRectRegion` y `XorRectRegion`: la región con un rectángulo.
    pub(crate) fn rect_region<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        reg: u32,
        rect: u32,
        op: Op,
    ) -> Result<u32, Fallo> {
        let r = self.leer_rectangulo(ex, rect)?;
        let rs = if r.vacio() { Vec::new() } else { alloc::vec![r] };
        self.operar_region(ex, reg, &rs, op)
    }

    /// `OrRegionRegion`, `AndRegionRegion` y `XorRegionRegion` (src en a0, dest en a1): `dest = dest op src`.
    pub(crate) fn region_region<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        src: u32,
        dest: u32,
        op: Op,
    ) -> Result<u32, Fallo> {
        let (rs, _) = self.leer_region(ex, src)?;
        self.operar_region(ex, dest, &rs, op)
    }

    /// Pone en una región del sistema (la `DamageList` de una capa) exactamente `rs`, con nodos del sistema.
    pub(crate) fn poner_region_sistema<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        reg: u32,
        rs: &[Rect],
    ) -> Result<bool, Fallo> {
        let (_, nodos) = self.leer_region(ex, reg)?;
        let rs = operar(rs, &[], Op::O).ok_or(Fallo::SinMemoria)?;
        self.escribir_region(ex, reg, &nodos, &rs, true)
    }

    /// Una región vacía en memoria del sistema (la `DamageList` de una capa).
    pub(crate) fn region_del_sistema<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Result<u32, Fallo> {
        let d = ex.reservar(region::TAMANO, memf::CLEAR)?;
        self.reg.regiones.insert(d, 0);
        Ok(d)
    }

    /// Vacía y suelta una región de `region_del_sistema` (0 no hace nada). Lo que falle no para el cierre de la
    /// ventana: se avisa.
    pub(crate) fn soltar_dano<M: Memoria>(&mut self, ex: &mut Exec<M>, reg: u32) {
        if reg == 0 {
            return;
        }
        if let Err(f) = self.poner_region_sistema(ex, reg, &[]) {
            self.avisar(ex, "CloseWindow", f);
        }
        self.reg.regiones.remove(&reg);
        if let Err(f) = ex.soltar(reg, region::TAMANO) {
            self.avisar(ex, "CloseWindow", f);
        }
    }

    /// Lo de las regiones de una tarea que se va: sus nodos y regiones los suelta la caja con su memoria.
    pub(crate) fn regiones_tarea_muerta(&mut self, t: u32) {
        self.reg.nodos.retain(|_, d| *d != t);
        self.reg.regiones.retain(|_, d| *d != t);
    }
}
