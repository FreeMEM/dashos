//! `ciaa.resource` y `ciab.resource` nativos, y los temporizadores A y B de las dos CIA (rama amiga-apps).
//!
//! Los reproductores de módulos (ProTracker, OctaMED) piden un temporizador de la CIA-B con `AddICRVector` y lo
//! programan escribiendo en sus registros: el periodo en `TALO`/`TAHI` (`$BFD400`, `$BFD500`) y la marcha en `CRA`
//! (`$BFDE00`). En la caja no hay CIA: lo que hay en esas direcciones es memoria reservada del sistema (el hueco del
//! chipset de exec, `HUECO_DEL_CHIPSET`), que `AddICRVector` hace existir (`cubrir_hasta`) antes de dar el
//! temporizador. La caja lee esos registros como los escribió el programa y llama a su interrupción a ese ritmo
//! (a 709.379 Hz por cuenta, el reloj E de un Amiga PAL), desde las llamadas nativas y la interrupción de nivel 6 de
//! la caja, como los ganchos de AHI (`sonar` en `lib.rs`).
//!
//! Escrito del autodoc de cia.resource (`AddICRVector`, `RemICRVector`, `AbleICR`, `SetICR`), `hardware/cia.h` y el
//! Hardware Reference Manual (los registros y sus direcciones). Sin código de nadie. Lo que no se imita: el modo de un
//! disparo (`RUNMODE`), el temporizador que cuenta los pulsos del otro, el reloj de cuadros (TOD), el puerto serie y
//! las líneas de los puertos: se apuntan como si no estuvieran.

use alloc::vec::Vec;
use caja_exec::{Exec, Fallo, Memoria, Registros};

use crate::memoria::MemoriaCaja;

pub const FUNCIONES: [&str; 4] = ["AddICRVector", "RemICRVector", "AbleICR", "SetICR"];
/// El reloj de las CIA (el E de un Amiga PAL), en cuentas por segundo.
const RELOJ_E: u64 = 709_379;
/// Donde empiezan los registros de cada CIA y lo que separa uno de otro (la A en las direcciones impares, la B en
/// las pares).
const BASE: [u32; 2] = [0x00BF_E001, 0x00BF_D000];
const PASO: u32 = 0x100;
const TALO: u32 = 4;
const TBLO: u32 = 6;
const CRA: u32 = 14;
const CRB: u32 = 15;
/// Hasta dónde tiene que llegar la memoria para que existan los registros de las dos.
const HASTA: u32 = 0x00C0_0000;
/// `struct Interrupt`: `is_Data` e `is_Code`.
const IS_DATA: u32 = 14;
const IS_CODE: u32 = 18;

pub struct Cias {
    pub bases: [u32; 2],
    n0: u16,
    /// La `struct Interrupt` de cada bit del ICR (0 TA, 1 TB, 2 ALRM, 3 SP, 4 FLG) y la tarea que la puso.
    vect: [[u32; 5]; 2],
    tareas: [[u32; 5]; 2],
    mascara: [u8; 2],
    pendiente: [u8; 2],
    /// Cuándo vence la siguiente cuenta de cada temporizador (µs), 0 si no está en marcha.
    proximo: [[u64; 2]; 2],
    /// Interrupciones dadas (para las medidas).
    pub dadas: u64,
}

impl Cias {
    pub fn instalar(ex: &mut Exec<MemoriaCaja>, n0: u16) -> Result<Cias, Fallo> {
        let a: Vec<u16> = (0..4).map(|i| n0 + i).collect();
        let b: Vec<u16> = (4..8).map(|i| n0 + i).collect();
        let ba = ex.crear_recurso_nativo("ciaa.resource", 40, &a)?;
        let bb = ex.crear_recurso_nativo("ciab.resource", 40, &b)?;
        Ok(Cias {
            bases: [ba, bb],
            n0,
            vect: [[0; 5]; 2],
            tareas: [[0; 5]; 2],
            mascara: [0; 2],
            pendiente: [0; 2],
            proximo: [[0; 2]; 2],
            dadas: 0,
        })
    }

    pub fn es_mio(&self, n: u16) -> bool {
        (self.n0..self.n0 + 8).contains(&n)
    }

    /// Si hay algún temporizador pedido (entonces la caja tiene que mirar el reloj aunque haya una sola tarea).
    pub fn activo(&self) -> bool {
        (0..2).any(|c| self.vect[c][0] != 0 || self.vect[c][1] != 0)
    }

    /// La función `n` con los registros `r` (D0 el resultado). Un error de memoria, `Err`.
    pub fn llamar(&mut self, ex: &mut Exec<MemoriaCaja>, n: u16, r: &mut Registros) -> Result<(), Fallo> {
        let i = (n - self.n0) as usize;
        let (cia, f) = (i / 4, i % 4);
        match f {
            // AddICRVector(resource, iCRBit, interrupt)(a6/d0/a1): 0 si es suyo; si ya era de otro, esa interrupción.
            0 => {
                let bit = r.d[0] as usize;
                if bit > 4 || r.a[1] == 0 {
                    r.d[0] = r.a[1].max(1);
                    return Err(Fallo::PunteroNulo);
                }
                ex.mem.leer32(r.a[1] + IS_CODE)?;
                if self.vect[cia][bit] != 0 {
                    r.d[0] = self.vect[cia][bit];
                    return Ok(());
                }
                // Sus registros tienen que existir (reservados) antes de que el programa los escriba; si la memoria
                // no puede llegar (el tope), el temporizador no se da.
                if !ex.cubrir_hasta(HASTA)? {
                    r.d[0] = r.a[1];
                    return Ok(());
                }
                self.vect[cia][bit] = r.a[1];
                self.tareas[cia][bit] = ex.tarea_actual();
                self.mascara[cia] |= 1 << bit;
                if bit < 2 {
                    self.proximo[cia][bit] = 0;
                }
                r.d[0] = 0;
            }
            // RemICRVector(resource, iCRBit, interrupt)(a6/d0/a1)
            1 => {
                let bit = r.d[0] as usize;
                if bit <= 4 && self.vect[cia][bit] == r.a[1] {
                    self.vect[cia][bit] = 0;
                    self.tareas[cia][bit] = 0;
                    self.mascara[cia] &= !(1 << bit);
                }
            }
            // AbleICR(resource, mask)(a6/d0): con el bit 7, enciende los bits de la máscara; sin él, los apaga.
            // Devuelve la máscara de antes.
            2 => {
                let antes = self.mascara[cia];
                let m = r.d[0] as u8;
                if m & 0x80 != 0 {
                    self.mascara[cia] |= m & 0x1F;
                } else {
                    self.mascara[cia] &= !(m & 0x1F);
                }
                r.d[0] = antes as u32;
            }
            // SetICR(resource, mask)(a6/d0): lo pendiente de antes; con el bit 7, se marca como pendiente.
            _ => {
                let antes = self.pendiente[cia];
                let m = r.d[0] as u8;
                if m & 0x80 != 0 {
                    self.pendiente[cia] |= m & 0x1F;
                } else {
                    self.pendiente[cia] &= !(m & 0x1F);
                }
                r.d[0] = antes as u32;
            }
        }
        Ok(())
    }

    /// Una tarea acaba: sus interrupciones se quitan (su código ya no estará).
    pub fn tarea_muerta(&mut self, t: u32) {
        for cia in 0..2 {
            for bit in 0..5 {
                if self.tareas[cia][bit] == t && self.vect[cia][bit] != 0 {
                    self.vect[cia][bit] = 0;
                    self.tareas[cia][bit] = 0;
                    self.mascara[cia] &= !(1 << bit);
                }
            }
        }
    }

    /// Si algún temporizador en marcha ya venció (para las esperas de la caja: entonces hay que dar el testigo a una
    /// tarea y llamar a su interrupción, aunque todas estén esperando). Pone en hora los que acaban de arrancar.
    pub fn hay_vencida(&mut self, ex: &Exec<MemoriaCaja>, ahora: u64) -> bool {
        let mut si = false;
        for cia in 0..2 {
            for t in 0..2 {
                if self.vect[cia][t] == 0 || self.mascara[cia] & (1 << t) == 0 {
                    continue;
                }
                let reg = |r: u32| ex.mem.leer8(BASE[cia] + r * PASO).unwrap_or(0);
                let (lo, cr) = if t == 0 { (TALO, CRA) } else { (TBLO, CRB) };
                let latch = reg(lo) as u64 | (reg(lo + 1) as u64) << 8;
                if reg(cr) & 1 == 0 || latch == 0 {
                    continue;
                }
                let p = &mut self.proximo[cia][t];
                if *p == 0 {
                    *p = ahora + (latch * 1_000_000 / RELOJ_E).max(100);
                } else if ahora >= *p {
                    si = true;
                }
            }
        }
        si
    }

    /// La siguiente interrupción que toca (su código y sus registros: A1 = `is_Data`, A5 = `is_Code`, A6 = ExecBase),
    /// o `None`. Una cuenta muy atrasada (la caja estuvo parada) no se recupera entera: se sigue desde ahora.
    pub fn vencida(&mut self, ex: &Exec<MemoriaCaja>, ahora: u64) -> Option<(u32, Registros)> {
        for cia in 0..2 {
            for t in 0..2 {
                let v = self.vect[cia][t];
                if v == 0 || self.mascara[cia] & (1 << t) == 0 {
                    self.proximo[cia][t] = 0;
                    continue;
                }
                let reg = |r: u32| ex.mem.leer8(BASE[cia] + r * PASO).unwrap_or(0);
                let (lo, cr) = if t == 0 { (TALO, CRA) } else { (TBLO, CRB) };
                let latch = reg(lo) as u64 | (reg(lo + 1) as u64) << 8;
                if reg(cr) & 1 == 0 || latch == 0 {
                    self.proximo[cia][t] = 0;
                    continue;
                }
                let periodo = (latch * 1_000_000 / RELOJ_E).max(100);
                let p = &mut self.proximo[cia][t];
                if *p == 0 {
                    *p = ahora + periodo;
                    continue;
                }
                if ahora < *p {
                    continue;
                }
                *p = if ahora > *p + 8 * periodo { ahora + periodo } else { *p + periodo };
                let (Ok(datos), Ok(codigo)) = (ex.mem.leer32(v + IS_DATA), ex.mem.leer32(v + IS_CODE)) else { continue };
                if codigo == 0 || codigo & 1 != 0 {
                    continue;
                }
                let mut r = Registros::default();
                r.a[1] = datos;
                r.a[5] = codigo;
                r.a[6] = ex.base();
                r.d[1] = 1 << t;
                self.dadas += 1;
                return Some((codigo, r));
            }
        }
        None
    }
}
