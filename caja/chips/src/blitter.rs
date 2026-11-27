//! El Blitter de OCS (S18, fase 2, adelantada): copia, relleno y líneas. El blit se hace entero al escribir `BLTSIZE`
//! y `BBUSY` sigue a 1 el tiempo que tardaría (S18, sección 3, "El Blitter acaba de golpe").
//!
//! This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not
//! distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.
//!
//! Fuentes: el Amiga Hardware Reference Manual (capítulo 6, el Blitter), de memoria, y el núcleo de vAmiga
//! (<https://github.com/dirkwhoffmann/vAmiga>, `Core/Components/Agnus/Blitter`, de Dirk W. Hoffmann, MPL-2.0).
//! De vAmiga salen, reescritos en Rust: el orden de cada palabra del blit de copia y el de cada punto del de líneas
//! (`FastBlitter.cpp`: `doFastCopyBlit` y `doFastLineBlit`), el desplazador y las tablas del relleno (`Blitter.cpp`:
//! `barrelShifter` y `_initialize`), lo que hace cada escritura de registro (`BlitterRegs.cpp`: los punteros pares,
//! los módulos pares, `BLTBDAT` que pasa por el desplazador al escribirlo y `BLTSIZE` con 0 = 1024 y 64) y los ciclos
//! por palabra según `USEx` (los microprogramas de `SlowBlitter.cpp`, que son la tabla del manual). No se ha leído
//! código de UAE, WinUAE, FS-UAE, Amiberry ni AROS.
//!
//! # Lo que se imita
//!
//! - **Copia** (`LINE` = 0): las fuentes A, B y C, cada una por DMA si tiene su `USEx` y si no, su registro de datos;
//!   las máscaras de primera y última palabra sobre A; los desplazamientos de A y B, con la palabra anterior; los
//!   minterms; los módulos al final de cada fila; el modo descendente (`DESC`: punteros y módulos hacia atrás, y los
//!   desplazamientos hacia la izquierda).
//! - **Relleno** (`IFE` o `EFE`, con o sin `DESC`): sobre la salida de los minterms, de derecha a izquierda dentro de
//!   cada palabra; el arrastre pasa de palabra a palabra y vuelve a `FCI` al empezar cada fila. Con los dos, el
//!   exclusivo.
//! - **Líneas** (`LINE` = 1): `ASH` es la posición del punto en la palabra y `BSH` la del patrón; el octante por
//!   `SUD`, `SUL` y `AUL`; el acumulador en `BLTAPT` (le suma `BLTBMOD` con `SIGN` y `BLTAMOD` sin él), `SIGN` es su
//!   bit 15; `SING`, un punto por fila; el punto sale de `BLTADAT` (con la máscara de primera palabra) desplazado por
//!   `ASH`, y el patrón, del bit `BSH` de `BLTBDAT`. La primera escritura va a `BLTDPT`; después, a donde apuntaba
//!   `BLTCPT` (el manual pide los dos punteros iguales al empezar). Sin `USEC` no escribe.
//! - `BZERO`: 1 si todas las palabras de D (escritas o no) han sido 0. `BBUSY` mientras dura.
//! - Los registros se quedan como los deja el blit: los punteros, `ASH`, `BSH` y `SIGN` en las líneas, y las
//!   palabras de A y B que se leyeron (un blit siguiente sin `USEA` usa la última A leída, no `BLTADAT`).
//!
//! # El tiempo
//!
//! Una estimación: los ciclos de bus por palabra según `USEx` (la tabla del manual: de 2 a 4), más 2 al empezar, a
//! 2 ciclos de CPU cada uno. En líneas, 4 ciclos de bus por punto. Sin robo de ciclos por los planos ni por la CPU, y
//! sin `BLTPRI`.
//!
//! # Sin DMA
//!
//! Si al escribir `BLTSIZE` `DMACON` no tiene `DMAEN` y `BLTEN`, el blit espera (con `BBUSY` a 1) a que se enciendan,
//! y se hace entonces con los registros que haya. Así lo hace el Amiga y vAmiga.
//!
//! Dudas: el manual no dice qué pasa si se escribe `BLTSIZE` con un blit aún en marcha. Aquí el anterior ya está
//! hecho en memoria: se da por acabado (sube `BLIT`) y empieza el nuevo. Tampoco qué hace un blit de líneas con
//! `USEB` (aquí lee B por DMA y avanza `BLTBPT` con `BLTBMOD`, como vAmiga) ni con un ancho distinto de 2 (aquí no
//! importa: un punto por fila de `BLTSIZE`).

use crate::memoria::MemoriaChip;

/// La chip RAM que ven los punteros de DMA: 512 KB de OCS. Los punteros tienen los bits 18 a 1 (en `BLTxPTH`, los
/// bits 2-0; en `BLTxPTL`, los 15-1).
pub const MASCARA_CHIP: u32 = 0x0007_FFFE;

/// Los registros del Blitter, por su desplazamiento desde `$DFF000`. Todos son de solo escritura.
pub mod reg {
    pub const BLTCON0: u32 = 0x040;
    pub const BLTCON1: u32 = 0x042;
    pub const BLTAFWM: u32 = 0x044;
    pub const BLTALWM: u32 = 0x046;
    pub const BLTCPTH: u32 = 0x048;
    pub const BLTCPTL: u32 = 0x04A;
    pub const BLTBPTH: u32 = 0x04C;
    pub const BLTBPTL: u32 = 0x04E;
    pub const BLTAPTH: u32 = 0x050;
    pub const BLTAPTL: u32 = 0x052;
    pub const BLTDPTH: u32 = 0x054;
    pub const BLTDPTL: u32 = 0x056;
    pub const BLTSIZE: u32 = 0x058;
    pub const BLTCMOD: u32 = 0x060;
    pub const BLTBMOD: u32 = 0x062;
    pub const BLTAMOD: u32 = 0x064;
    pub const BLTDMOD: u32 = 0x066;
    pub const BLTCDAT: u32 = 0x070;
    pub const BLTBDAT: u32 = 0x072;
    pub const BLTADAT: u32 = 0x074;
}

/// Los bits de `BLTCON0`. Los 15-12 son `ASH` (en líneas, la posición del punto) y los 7-0, los minterms.
pub mod con0 {
    pub const USEA: u16 = 0x0800;
    pub const USEB: u16 = 0x0400;
    pub const USEC: u16 = 0x0200;
    pub const USED: u16 = 0x0100;
}

/// Los bits de `BLTCON1`. Los 15-12 son `BSH` (en líneas, la posición en el patrón).
pub mod con1 {
    /// Modo línea.
    pub const LINE: u16 = 0x0001;
    /// Copia: descendente.
    pub const DESC: u16 = 0x0002;
    /// Copia: el arrastre del relleno al empezar cada fila.
    pub const FCI: u16 = 0x0004;
    /// Copia: relleno inclusivo.
    pub const IFE: u16 = 0x0008;
    /// Copia: relleno exclusivo.
    pub const EFE: u16 = 0x0010;
    /// Línea: un punto por fila.
    pub const SING: u16 = 0x0002;
    /// Línea: el paso de siempre va hacia arriba o a la izquierda.
    pub const AUL: u16 = 0x0004;
    /// Línea: el paso de a veces va hacia arriba o a la izquierda.
    pub const SUL: u16 = 0x0008;
    /// Línea: el paso de a veces es vertical (el de siempre, horizontal).
    pub const SUD: u16 = 0x0010;
    /// Línea: el acumulador es negativo.
    pub const SIGN: u16 = 0x0040;
}

/// Ciclos de bus por palabra según `USEA`, `USEB`, `USEC` y `USED` (bits 3 a 0 del índice), del manual.
const CICLOS_POR_PALABRA: [u64; 16] = [2, 2, 2, 3, 3, 3, 3, 4, 2, 2, 2, 3, 3, 3, 3, 4];
/// Ciclos de bus al empezar un blit.
const CICLOS_AL_EMPEZAR: u64 = 2;
/// Ciclos de bus por punto de una línea.
const CICLOS_POR_PUNTO: u64 = 4;
/// Ciclos de CPU por ciclo de bus.
const CPU_POR_BUS: u64 = crate::CICLOS_POR_COLOR;

/// El Blitter: sus registros, lo que guarda de un blit al siguiente y el que está en marcha.
#[derive(Clone, Debug, Default)]
pub struct Blitter {
    bltcon0: u16,
    bltcon1: u16,
    afwm: u16,
    alwm: u16,
    apt: u32,
    bpt: u32,
    cpt: u32,
    dpt: u32,
    amod: i16,
    bmod: i16,
    cmod: i16,
    dmod: i16,
    /// La última palabra de A: `BLTADAT` o la leída por DMA.
    anew: u16,
    /// La última de B: `BLTBDAT` o la leída por DMA.
    bnew: u16,
    /// La palabra anterior de A, con su máscara, para el desplazamiento.
    aold: u16,
    /// La anterior de B.
    bold: u16,
    /// B ya desplazada: la que usan los minterms.
    bhold: u16,
    /// C: `BLTCDAT` o la leída por DMA.
    chold: u16,
    /// El `BLTSIZE` escrito: el blit que espera al DMA o el último hecho.
    tamano: u16,
    /// `BBUSY`.
    ocupado: bool,
    /// Escrito `BLTSIZE` sin DMA del Blitter: aún no se ha hecho.
    pendiente: bool,
    /// `BZERO`.
    cero: bool,
    /// Si está ocupado y no pendiente: la hora en que acaba.
    fin: u64,
}

impl Blitter {
    /// Escribe un registro del Blitter que no arranca nada. Dice si `off` es uno de ellos.
    pub fn escribir_registro(&mut self, off: u32, v: u16) -> bool {
        let alto = |p: u32| (p & 0xFFFF) | (u32::from(v) << 16 & MASCARA_CHIP);
        let bajo = |p: u32| (p & !0xFFFF) | u32::from(v & 0xFFFE);
        match off {
            reg::BLTCON0 => self.bltcon0 = v,
            reg::BLTCON1 => self.bltcon1 = v,
            reg::BLTAFWM => self.afwm = v,
            reg::BLTALWM => self.alwm = v,
            reg::BLTCPTH => self.cpt = alto(self.cpt),
            reg::BLTCPTL => self.cpt = bajo(self.cpt),
            reg::BLTBPTH => self.bpt = alto(self.bpt),
            reg::BLTBPTL => self.bpt = bajo(self.bpt),
            reg::BLTAPTH => self.apt = alto(self.apt),
            reg::BLTAPTL => self.apt = bajo(self.apt),
            reg::BLTDPTH => self.dpt = alto(self.dpt),
            reg::BLTDPTL => self.dpt = bajo(self.dpt),
            reg::BLTCMOD => self.cmod = (v & 0xFFFE) as i16,
            reg::BLTBMOD => self.bmod = (v & 0xFFFE) as i16,
            reg::BLTAMOD => self.amod = (v & 0xFFFE) as i16,
            reg::BLTDMOD => self.dmod = (v & 0xFFFE) as i16,
            reg::BLTCDAT => self.chold = v,
            reg::BLTBDAT => {
                // Escribir BLTBDAT pasa la palabra por el desplazador de B (BLTADAT no); un blit sin USEB usa esa.
                self.bnew = v;
                self.bhold = desplazar(v, self.bold, self.bsh(), self.bltcon1 & con1::DESC != 0);
                self.bold = v;
            }
            reg::BLTADAT => self.anew = v,
            _ => return false,
        }
        true
    }

    /// Escribe `BLTSIZE` a la hora `hora`: con el DMA del Blitter (`dma`), el blit entero; sin él, espera.
    /// Devuelve si se dio por acabado un blit que aún estaba en marcha (para subir su `BLIT`).
    pub fn escribir_tamano(&mut self, v: u16, hora: u64, dma: bool, mem: &mut dyn MemoriaChip) -> bool {
        let anterior = self.ocupado && !self.pendiente;
        self.tamano = v;
        self.ocupado = true;
        self.pendiente = true;
        if dma {
            self.hacer(hora, mem);
        }
        anterior
    }

    /// El DMA del Blitter se acaba de encender a la hora `hora`: si había un blit esperando, se hace.
    pub fn dma_encendido(&mut self, hora: u64, mem: &mut dyn MemoriaChip) {
        if self.pendiente {
            self.hacer(hora, mem);
        }
    }

    /// Pone el Blitter al día hasta `hora`. Dice si ha acabado un blit (`BBUSY` baja y sube `BLIT`).
    pub fn avanzar(&mut self, hora: u64) -> bool {
        if self.ocupado && !self.pendiente && hora >= self.fin {
            self.ocupado = false;
            return true;
        }
        false
    }

    /// La hora en que acaba el blit en marcha, si hay uno (no uno que espera al DMA).
    pub fn siguiente_suceso(&self) -> Option<u64> {
        (self.ocupado && !self.pendiente).then_some(self.fin)
    }

    /// `BBUSY` (bit 14) y `BZERO` (bit 13), para `DMACONR`.
    pub fn bits_dmaconr(&self) -> u16 {
        (u16::from(self.ocupado) << 14) | (u16::from(self.cero) << 13)
    }

    fn ash(&self) -> u16 {
        self.bltcon0 >> 12
    }

    fn bsh(&self) -> u16 {
        self.bltcon1 >> 12
    }

    /// Filas (bits 15-6, 0 = 1024) y palabras (bits 5-0, 0 = 64) de `BLTSIZE`.
    fn medidas(&self) -> (u32, u32) {
        let filas = u32::from(self.tamano >> 6);
        let palabras = u32::from(self.tamano & 0x3F);
        (if filas == 0 { 1024 } else { filas }, if palabras == 0 { 64 } else { palabras })
    }

    fn hacer(&mut self, hora: u64, mem: &mut dyn MemoriaChip) {
        self.pendiente = false;
        self.cero = true;
        let (filas, palabras) = self.medidas();
        let ciclos = if self.bltcon1 & con1::LINE != 0 {
            self.linea(filas, mem);
            u64::from(filas) * CICLOS_POR_PUNTO
        } else {
            self.copia(filas, palabras, mem);
            u64::from(filas * palabras) * CICLOS_POR_PALABRA[usize::from(self.bltcon0 >> 8 & 0xF)]
        };
        self.fin = hora + (CICLOS_AL_EMPEZAR + ciclos) * CPU_POR_BUS;
    }

    /// El blit de copia (con relleno o sin él), palabra a palabra.
    fn copia(&mut self, filas: u32, palabras: u32, mem: &mut dyn MemoriaChip) {
        let usa = |bit| self.bltcon0 & bit != 0;
        let (usa_a, usa_b, usa_c, usa_d) = (usa(con0::USEA), usa(con0::USEB), usa(con0::USEC), usa(con0::USED));
        let desc = self.bltcon1 & con1::DESC != 0;
        let relleno = self.bltcon1 & (con1::IFE | con1::EFE) != 0;
        let exclusivo = self.bltcon1 & con1::EFE != 0;
        let (ash, bsh, lf) = (self.ash(), self.bsh(), self.bltcon0 as u8);
        let paso: u32 = if desc { 2u32.wrapping_neg() } else { 2 };
        let modulo = |m: i16| if desc { (m as u32).wrapping_neg() } else { m as u32 };
        let (amod, bmod, cmod, dmod) = (modulo(self.amod), modulo(self.bmod), modulo(self.cmod), modulo(self.dmod));

        self.aold = 0;
        self.bold = 0;
        for _ in 0..filas {
            let mut arrastre = self.bltcon1 & con1::FCI != 0;
            let mut mascara = self.afwm;
            for x in 0..palabras {
                if x == palabras - 1 {
                    mascara &= self.alwm;
                }
                if usa_a {
                    self.anew = mem.leer_palabra(self.apt & MASCARA_CHIP);
                    self.apt = self.apt.wrapping_add(paso);
                }
                if usa_b {
                    self.bnew = mem.leer_palabra(self.bpt & MASCARA_CHIP);
                    self.bpt = self.bpt.wrapping_add(paso);
                }
                if usa_c {
                    self.chold = mem.leer_palabra(self.cpt & MASCARA_CHIP);
                    self.cpt = self.cpt.wrapping_add(paso);
                }
                // A pasa por el desplazador aunque no tenga DMA; B, solo con DMA (si no, ya pasó al escribir
                // BLTBDAT).
                let ahold = desplazar(self.anew & mascara, self.aold, ash, desc);
                self.aold = self.anew & mascara;
                if usa_b {
                    self.bhold = desplazar(self.bnew, self.bold, bsh, desc);
                    self.bold = self.bnew;
                }
                let mut d = minterms(ahold, self.bhold, self.chold, lf);
                if relleno {
                    d = rellenar(d, &mut arrastre, exclusivo);
                }
                if d != 0 {
                    self.cero = false;
                }
                if usa_d {
                    mem.escribir_palabra(self.dpt & MASCARA_CHIP, d);
                    self.dpt = self.dpt.wrapping_add(paso);
                }
                mascara = 0xFFFF;
            }
            if usa_a {
                self.apt = self.apt.wrapping_add(amod);
            }
            if usa_b {
                self.bpt = self.bpt.wrapping_add(bmod);
            }
            if usa_c {
                self.cpt = self.cpt.wrapping_add(cmod);
            }
            if usa_d {
                self.dpt = self.dpt.wrapping_add(dmod);
            }
        }
    }

    /// El blit de líneas: un punto por fila de `BLTSIZE`.
    fn linea(&mut self, filas: u32, mem: &mut dyn MemoriaChip) {
        let con1 = self.bltcon1;
        let usa_a = self.bltcon0 & con0::USEA != 0;
        let usa_b = self.bltcon0 & con0::USEB != 0;
        let usa_c = self.bltcon0 & con0::USEC != 0;
        let sing = con1 & con1::SING != 0;
        let sud = con1 & con1::SUD != 0;
        let sul = con1 & con1::SUL != 0;
        let aul = con1 & con1::AUL != 0;
        let lf = self.bltcon0 as u8;
        let mut signo = con1 & con1::SIGN != 0;
        let mut ash = self.ash();
        let mut bsh = self.bsh();
        // Un punto nuevo en la fila (para SING): el primero y el siguiente a cada paso vertical.
        let mut primero = true;

        for _ in 0..filas {
            if usa_b {
                self.bnew = mem.leer_palabra(self.bpt & MASCARA_CHIP);
                self.bpt = self.bpt.wrapping_add(self.bmod as u32);
            }
            if usa_c {
                self.chold = mem.leer_palabra(self.cpt & MASCARA_CHIP);
            }
            let ahold = (self.anew & self.afwm) >> ash;
            // El patrón: el bit BSH de B; el siguiente punto usa el de la derecha.
            self.bhold = self.bnew.rotate_right(u32::from(bsh));
            bsh = bsh.wrapping_sub(1) & 0xF;
            let d = minterms(ahold, if self.bhold & 1 != 0 { 0xFFFF } else { 0 }, self.chold, lf);
            let escribe = (!sing || primero) && usa_c;

            // El paso: el de a veces si el acumulador no es negativo, y el de siempre.
            primero = false;
            let mut x = |izquierda: bool, cpt: &mut u32| {
                if izquierda {
                    if ash == 0 {
                        *cpt = cpt.wrapping_sub(2);
                    }
                    ash = ash.wrapping_sub(1) & 0xF;
                } else {
                    ash = (ash + 1) & 0xF;
                    if ash == 0 {
                        *cpt = cpt.wrapping_add(2);
                    }
                }
            };
            let cmod = self.cmod as u32;
            let y = |arriba: bool, cpt: &mut u32| {
                *cpt = if arriba { cpt.wrapping_sub(cmod) } else { cpt.wrapping_add(cmod) };
            };
            if !signo {
                if sud {
                    y(sul, &mut self.cpt);
                    primero = true;
                } else {
                    x(sul, &mut self.cpt);
                }
            }
            if sud {
                x(aul, &mut self.cpt);
            } else {
                y(aul, &mut self.cpt);
                primero = true;
            }
            if usa_a {
                let m = if signo { self.bmod } else { self.amod };
                self.apt = self.apt.wrapping_add(m as u32);
            }
            signo = (self.apt as u16 as i16) < 0;

            if d != 0 {
                self.cero = false;
            }
            if escribe {
                mem.escribir_palabra(self.dpt & MASCARA_CHIP, d);
            }
            self.dpt = self.cpt;
        }
        self.bltcon0 = (self.bltcon0 & 0x0FFF) | ash << 12;
        self.bltcon1 = (self.bltcon1 & 0x0FBF) | bsh << 12 | if signo { con1::SIGN } else { 0 };
    }
}

/// El desplazador: hacia la derecha `n` bits con la palabra anterior detrás, o en descendente hacia la izquierda con
/// la anterior delante.
fn desplazar(nueva: u16, anterior: u16, n: u16, desc: bool) -> u16 {
    let (nueva, anterior) = (u32::from(nueva), u32::from(anterior));
    if desc { ((nueva << 16 | anterior) >> (16 - n)) as u16 } else { ((anterior << 16 | nueva) >> n) as u16 }
}

/// Los minterms: el bit 7 de `lf` es ABC, el 6 ABc, ... el 0 abc (minúscula, negada).
fn minterms(a: u16, b: u16, c: u16, lf: u8) -> u16 {
    let mut d = 0;
    for i in 0..8 {
        if lf & (1 << i) != 0 {
            let a = if i & 4 != 0 { a } else { !a };
            let b = if i & 2 != 0 { b } else { !b };
            let c = if i & 1 != 0 { c } else { !c };
            d |= a & b & c;
        }
    }
    d
}

/// El relleno de una palabra, del bit 0 al 15. Cada 1 cambia el arrastre. Inclusivo: sale el 1 o el arrastre de
/// antes. Exclusivo: sale el arrastre de después (el 1 de la izquierda de cada tramo no sale).
fn rellenar(d: u16, arrastre: &mut bool, exclusivo: bool) -> u16 {
    let mut r = d;
    for bit in 0..16 {
        let m = 1u16 << bit;
        if *arrastre {
            if exclusivo { r ^= m } else { r |= m }
        }
        if d & m != 0 {
            *arrastre = !*arrastre;
        }
    }
    r
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn minterms_de_cada_fuente() {
        assert_eq!(minterms(0x1234, 0, 0, 0xF0), 0x1234);
        assert_eq!(minterms(0, 0x1234, 0, 0xCC), 0x1234);
        assert_eq!(minterms(0, 0, 0x1234, 0xAA), 0x1234);
        assert_eq!(minterms(0xFF00, 0x0FF0, 0xAAAA, 0xCA), 0xFF00 & 0x0FF0 | 0x00FF & 0xAAAA);
        assert_eq!(minterms(1, 2, 4, 0x96), 7);
    }

    #[test]
    fn relleno_inclusivo_y_exclusivo() {
        let mut c = false;
        assert_eq!(rellenar(0b0010_0100, &mut c, false), 0b0011_1100);
        assert!(!c);
        assert_eq!(rellenar(0b0010_0100, &mut c, true), 0b0001_1100);
        let mut c = true;
        assert_eq!(rellenar(0b0000_0100, &mut c, false), 0b0000_0111);
        assert!(!c);
        let mut c = false;
        assert_eq!(rellenar(0x0001, &mut c, false), 0xFFFF);
        assert!(c);
    }

    #[test]
    fn desplazador() {
        assert_eq!(desplazar(0x1234, 0xABCD, 4, false), 0xD123);
        assert_eq!(desplazar(0x1234, 0xABCD, 4, true), 0x234A);
        assert_eq!(desplazar(0x1234, 0xABCD, 0, true), 0x1234);
        assert_eq!(desplazar(0x1234, 0xABCD, 0, false), 0x1234);
    }
}
