//! La V3D mediada (S25, camino C de `docs/diseño/S25-gpu-caja-linux.md`): lo que hace DashOS cuando el invitado toca
//! los registros de la V3D de la Pi 4, y la tabla de páginas sombra.
//!
//! La V3D ve la RAM física entera y la BCM2711 no tiene IOMMU delante (`docs/spec/hw/v3d.md`, 2.6): lo único que
//! confina sus accesos es su MMU. Así que el invitado nunca la programa. Sus accesos a los registros de la V3D salen a
//! DashOS (como un aparato emulado) y pasan por [`Mediador`]:
//!
//! - **La página de la MMU del hub** (`0x1000` a `0x1FFF`) es de DashOS. El invitado ve sus propios valores de
//!   `MMU_PT_PA_BASE`, `MMU_CTL`, `MMU_ILLEGAL_ADDR` y `MMUC_CONTROL`; a la V3D le llegan siempre la tabla sombra, la
//!   MMU encendida con los abortos puestos y una página de relleno de verdad. Las zonas sin traducir
//!   (`MMU_BYPASS_START`/`END`), el tope y el derribo de una página no se dejan tocar. Lo que no se conoce, tampoco.
//! - **El GMP del núcleo** (`0x800` a `0x8FF`): solo se lee su estado y se deja pedir que pare (`STOP_REQ`).
//! - **Lo demás** (identificación, interrupciones, cachés, listas de control, TFU, cómputo, contadores) pasa tal cual:
//!   las direcciones que llevan son de la MMU, y la MMU es la sombra.
//!
//! **La tabla sombra.** La del invitado vive en su RAM (su `MMU_PT_PA_BASE` es una dirección del invitado, IPA). Cuando
//! el invitado vacía la caché de la MMU o la TLB (Linux lo hace siempre tras cambiar entradas, `v3d.md`, 5.6), DashOS
//! lee la tabla del invitado una vez, traduce cada entrada válida (página del invitado → página física, con
//! [`Traductor`]) y escribe en la sombra solo lo que cambió. Una entrada que apunta fuera de la RAM del invitado queda
//! como no válida (la V3D la manda a la página de relleno y avisa). Se quitan los bits de página grande (con entradas
//! de 4 KB todo funciona igual, `v3d.md`, 5.2) y los que no se conocen. Lo que el invitado cambie después en su tabla
//! no llega a la V3D hasta el siguiente vaciado: la V3D solo lee la sombra.
//!
//! Lógica pura, sin `unsafe`: el hardware y la traducción van por traits. Lo de verdad (el servicio `gpu` con la V3D, la
//! llamada del kernel que da las páginas físicas de la RAM del invitado) solo existe en una Pi 4: sin probar.

use alloc::vec::Vec;

use crate::memoria::MemoriaInvitado;

/// Entradas de la tabla (4 GB de direcciones de la V3D en páginas de 4 KB) y su tamaño en bytes.
pub const ENTRADAS: usize = 1 << 20;
pub const TAM_TABLA: u64 = 4 << 20;
/// Entradas por página de 4 KB de la tabla (lo que se repasa de una vez).
pub const POR_PAGINA: usize = 1024;

pub const PTE_SUPERPAGE: u32 = 1 << 31;
pub const PTE_BIGPAGE: u32 = 1 << 30;
pub const PTE_WRITEABLE: u32 = 1 << 29;
pub const PTE_VALID: u32 = 1 << 28;
pub const PTE_PAGINA: u32 = 0x00ff_ffff;

// Registros del hub (`v3d.md`, 4.1 y 5.3).
pub const MMUC_CONTROL: u64 = 0x1000;
pub const MMU_CTL: u64 = 0x1200;
pub const MMU_PT_PA_BASE: u64 = 0x1204;
pub const MMU_HIT: u64 = 0x1208;
pub const MMU_MISSES: u64 = 0x120c;
pub const MMU_STALLS: u64 = 0x1210;
pub const MMU_ADDR_CAP: u64 = 0x1214;
pub const MMU_SHOOT_DOWN: u64 = 0x1218;
pub const MMU_BYPASS_START: u64 = 0x121c;
pub const MMU_BYPASS_END: u64 = 0x1220;
pub const MMU_VIO_ID: u64 = 0x122c;
pub const MMU_ILLEGAL_ADDR: u64 = 0x1230;
pub const MMU_VIO_ADDR: u64 = 0x1234;
pub const MMU_DEBUG_INFO: u64 = 0x1238;
// Del núcleo: el GMP.
pub const GMP_STATUS: u64 = 0x800;
pub const GMP_CFG: u64 = 0x804;
pub const GMP_VIO_ADDR: u64 = 0x808;

// Bits de MMU_CTL.
pub const CTL_ENABLE: u32 = 1 << 0;
pub const CTL_TLB_STATS_ENABLE: u32 = 1 << 1;
pub const CTL_TLB_CLEAR: u32 = 1 << 2;
pub const CTL_TLB_STATS_CLEAR: u32 = 1 << 3;
pub const CTL_TLB_CLEARING: u32 = 1 << 7;
pub const CTL_WRITE_VIOLATION_INT: u32 = 1 << 10;
pub const CTL_WRITE_VIOLATION_ABORT: u32 = 1 << 11;
pub const CTL_WRITE_VIOLATION: u32 = 1 << 12;
pub const CTL_PT_INVALID_ENABLE: u32 = 1 << 16;
pub const CTL_PT_INVALID_INT: u32 = 1 << 18;
pub const CTL_PT_INVALID_ABORT: u32 = 1 << 19;
pub const CTL_PT_INVALID: u32 = 1 << 20;
pub const CTL_CAP_EXCEEDED_INT: u32 = 1 << 25;
pub const CTL_CAP_EXCEEDED_ABORT: u32 = 1 << 26;
pub const CTL_CAP_EXCEEDED: u32 = 1 << 27;
/// Lo que la V3D tiene siempre en MMU_CTL, diga lo que diga el invitado: la MMU encendida, comprobando las entradas y
/// abortando lo malo.
pub const CTL_SEGURO: u32 = CTL_ENABLE | CTL_PT_INVALID_ENABLE | CTL_PT_INVALID_ABORT | CTL_WRITE_VIOLATION_ABORT | CTL_CAP_EXCEEDED_ABORT;
/// Bits de estado (se borran escribiendo 1, `v3d.md`, 5.7): pasan.
const CTL_ESTADO: u32 = CTL_WRITE_VIOLATION | CTL_PT_INVALID | CTL_CAP_EXCEEDED;
/// Lo que el invitado elige de verdad: avisar o no, y las estadísticas de la TLB.
const CTL_SUYO: u32 = CTL_WRITE_VIOLATION_INT | CTL_PT_INVALID_INT | CTL_CAP_EXCEEDED_INT | CTL_TLB_STATS_ENABLE;

pub const MMUC_ENABLE: u32 = 1 << 0;
pub const MMUC_FLUSH: u32 = 1 << 1;
pub const MMUC_FLUSHING: u32 = 1 << 2;

pub const ILLEGAL_ENABLE: u32 = 1 << 31;
pub const GMP_STOP_REQ: u32 = 1 << 1;

/// Los dos bloques de registros de la V3D 4.2 (16 KB cada uno).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bloque {
    Hub,
    Nucleo,
}

pub const TAM_BLOQUE: u64 = 0x4000;

/// La V3D de verdad, por el lado de DashOS (el servicio `gpu`).
pub trait V3dReal {
    fn leer(&mut self, bloque: Bloque, desp: u64) -> u32;
    fn escribir(&mut self, bloque: Bloque, desp: u64, valor: u32);
    /// Escribe la entrada `n` de la tabla sombra (memoria sin caché que ve la V3D).
    fn escribir_sombra(&mut self, n: usize, valor: u32);
    /// Página física (dirección >> 12) de la tabla sombra (4 MB seguidos) y de la página de relleno de DashOS.
    fn pagina_sombra(&self) -> u32;
    fn pagina_relleno(&self) -> u32;
}

/// Qué página física hay detrás de una página de la RAM del invitado (dirección del invitado >> 12). `None` si no es
/// de su RAM.
pub trait Traductor {
    fn pagina_fisica(&self, pagina_ipa: u64) -> Option<u64>;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cuentas {
    /// Vaciados del invitado que llevaron a repasar su tabla.
    pub repasos: u64,
    /// Entradas de la sombra que cambiaron.
    pub cambiadas: u64,
    /// Entradas válidas del invitado que apuntaban fuera de su RAM (quedan no válidas).
    pub fuera: u64,
    /// Escrituras a registros que no se dejan tocar (zonas sin traducir, tope, derribo, GMP, desconocidos).
    pub prohibidas: u64,
    /// Bases de tabla que no caben en la RAM del invitado.
    pub tablas_malas: u64,
}

pub struct Mediador {
    /// Lo que el invitado cree que hay (lo que escribió).
    pt_base: u32,
    ctl: u32,
    mmuc: u32,
    ilegal: u32,
    /// Si su tabla cabe en su RAM (si no, la sombra está vacía).
    tabla_buena: bool,
    /// Copia de la sombra, para escribir solo lo que cambia.
    sombra: Vec<u32>,
    pub cuentas: Cuentas,
}

impl Default for Mediador {
    fn default() -> Self {
        Self::nuevo()
    }
}

impl Mediador {
    pub fn nuevo() -> Self {
        Self { pt_base: 0, ctl: 0, mmuc: 0, ilegal: 0, tabla_buena: false, sombra: alloc::vec![0; ENTRADAS], cuentas: Cuentas::default() }
    }

    /// Programa la MMU de la V3D con la sombra (al dar la V3D a la caja y tras cada reinicio de la V3D). La sombra
    /// empieza vacía: todo no válido.
    pub fn programar(&mut self, hw: &mut dyn V3dReal) {
        for n in 0..ENTRADAS {
            if self.sombra[n] != 0 {
                hw.escribir_sombra(n, self.sombra[n]);
            }
        }
        hw.escribir(Bloque::Hub, MMU_PT_PA_BASE, hw.pagina_sombra());
        hw.escribir(Bloque::Hub, MMU_CTL, CTL_SEGURO | (self.ctl & CTL_SUYO));
        let relleno = self.relleno_fisico(hw.pagina_relleno(), None);
        hw.escribir(Bloque::Hub, MMU_ILLEGAL_ADDR, relleno);
        hw.escribir(Bloque::Hub, MMUC_CONTROL, MMUC_ENABLE);
        hw.escribir(Bloque::Hub, MMUC_CONTROL, MMUC_ENABLE | MMUC_FLUSH);
        let ctl = CTL_SEGURO | (self.ctl & CTL_SUYO) | CTL_TLB_CLEAR;
        hw.escribir(Bloque::Hub, MMU_CTL, ctl);
    }

    /// La entrada `n` de la sombra (para las pruebas y la depuración).
    pub fn sombra(&self, n: usize) -> u32 {
        self.sombra.get(n).copied().unwrap_or(0)
    }

    fn relleno_fisico(&self, de_dashos: u32, t: Option<&dyn Traductor>) -> u32 {
        // La página de relleno del invitado si es de su RAM; si no, la de DashOS. Siempre encendida (si no, un acceso
        // malo iría a la dirección física 0).
        let suya = (self.ilegal & ILLEGAL_ENABLE != 0)
            .then_some(())
            .and(t)
            .and_then(|t| t.pagina_fisica((self.ilegal & !ILLEGAL_ENABLE) as u64))
            .filter(|&p| p <= PTE_PAGINA as u64);
        ILLEGAL_ENABLE | suya.map_or(de_dashos & PTE_PAGINA, |p| p as u32)
    }

    /// Una lectura del invitado en `desp` de `bloque`.
    pub fn leer(&mut self, hw: &mut dyn V3dReal, bloque: Bloque, desp: u64) -> u32 {
        if desp >= TAM_BLOQUE || desp & 3 != 0 {
            return 0;
        }
        match (bloque, desp) {
            (Bloque::Hub, MMU_PT_PA_BASE) => self.pt_base,
            (Bloque::Hub, MMU_ILLEGAL_ADDR) => self.ilegal,
            (Bloque::Hub, MMU_CTL) => {
                // Su configuración, con el estado de verdad (borrando la TLB, fallos).
                let hw_ctl = hw.leer(Bloque::Hub, MMU_CTL);
                (self.ctl & !(CTL_ESTADO | CTL_TLB_CLEARING | CTL_TLB_CLEAR)) | (hw_ctl & (CTL_ESTADO | CTL_TLB_CLEARING))
            }
            (Bloque::Hub, MMUC_CONTROL) => (self.mmuc & MMUC_ENABLE) | (hw.leer(Bloque::Hub, MMUC_CONTROL) & MMUC_FLUSHING),
            (Bloque::Hub, MMU_HIT | MMU_MISSES | MMU_STALLS | MMU_VIO_ID | MMU_VIO_ADDR | MMU_DEBUG_INFO) => hw.leer(bloque, desp),
            (Bloque::Hub, 0x1000..=0x3fff) => 0,
            (Bloque::Nucleo, GMP_STATUS | GMP_CFG | GMP_VIO_ADDR) => hw.leer(bloque, desp),
            (Bloque::Nucleo, 0x800..=0x8ff) => 0,
            _ => hw.leer(bloque, desp),
        }
    }

    /// Una escritura del invitado. `mem` es su RAM (para leer su tabla) y `t`, la traducción a páginas físicas.
    pub fn escribir(&mut self, hw: &mut dyn V3dReal, mem: &dyn MemoriaInvitado, t: &dyn Traductor, bloque: Bloque, desp: u64, v: u32) {
        if desp >= TAM_BLOQUE || desp & 3 != 0 {
            self.cuentas.prohibidas += 1;
            return;
        }
        match (bloque, desp) {
            (Bloque::Hub, MMU_PT_PA_BASE) => {
                self.pt_base = v;
                // La tabla entera (4 MB) tiene que estar en su RAM. Si no, la sombra se queda vacía.
                let base = (v as u64) << 12;
                let mut b = [0u8; 1];
                self.tabla_buena = mem.leer(base, &mut b).is_ok() && mem.leer(base + TAM_TABLA - 1, &mut b).is_ok();
                if !self.tabla_buena {
                    self.cuentas.tablas_malas += 1;
                }
            }
            (Bloque::Hub, MMU_ILLEGAL_ADDR) => {
                self.ilegal = v;
                let r = self.relleno_fisico(hw.pagina_relleno(), Some(t));
                hw.escribir(Bloque::Hub, MMU_ILLEGAL_ADDR, r);
            }
            (Bloque::Hub, MMU_CTL) => {
                self.ctl = v & !(CTL_ESTADO | CTL_TLB_CLEAR | CTL_TLB_STATS_CLEAR);
                if v & CTL_TLB_CLEAR != 0 {
                    self.repasar(hw, mem, t);
                }
                let pasa = v & (CTL_ESTADO | CTL_TLB_CLEAR | CTL_TLB_STATS_CLEAR);
                hw.escribir(Bloque::Hub, MMU_CTL, CTL_SEGURO | (v & CTL_SUYO) | pasa);
            }
            (Bloque::Hub, MMUC_CONTROL) => {
                self.mmuc = v & MMUC_ENABLE;
                if v & MMUC_FLUSH != 0 {
                    self.repasar(hw, mem, t);
                }
                hw.escribir(Bloque::Hub, MMUC_CONTROL, MMUC_ENABLE | (v & MMUC_FLUSH));
            }
            (Bloque::Hub, 0x1000..=0x3fff) => self.cuentas.prohibidas += 1,
            (Bloque::Nucleo, GMP_CFG) => {
                // Solo pedir que pare (para apagar): proteger o no, y su tabla, son de DashOS.
                if v & !GMP_STOP_REQ != 0 {
                    self.cuentas.prohibidas += 1;
                }
                hw.escribir(bloque, desp, v & GMP_STOP_REQ);
            }
            (Bloque::Nucleo, 0x800..=0x8ff) => self.cuentas.prohibidas += 1,
            _ => hw.escribir(bloque, desp, v),
        }
    }

    /// La entrada de la sombra para una del invitado.
    fn traducir(&mut self, e: u32, t: &dyn Traductor) -> u32 {
        if e & PTE_VALID == 0 {
            return 0;
        }
        match t.pagina_fisica((e & PTE_PAGINA) as u64).filter(|&p| p <= PTE_PAGINA as u64) {
            Some(p) => (e & (PTE_VALID | PTE_WRITEABLE)) | p as u32,
            None => {
                self.cuentas.fuera += 1;
                0
            }
        }
    }

    /// Repasa la tabla del invitado entera y pone la sombra al día. Devuelve cuántas entradas cambiaron.
    pub fn repasar(&mut self, hw: &mut dyn V3dReal, mem: &dyn MemoriaInvitado, t: &dyn Traductor) -> usize {
        self.repasar_paginas(hw, mem, t, 0..ENTRADAS / POR_PAGINA)
    }

    /// Lo mismo, solo con las páginas `paginas` de la tabla del invitado (las que escribió, si el kernel lo sabe por
    /// la protección de escritura de la etapa 2).
    pub fn repasar_paginas(
        &mut self,
        hw: &mut dyn V3dReal,
        mem: &dyn MemoriaInvitado,
        t: &dyn Traductor,
        paginas: impl IntoIterator<Item = usize>,
    ) -> usize {
        self.cuentas.repasos += 1;
        let base = (self.pt_base as u64) << 12;
        let mut cambiadas = 0;
        let mut buf = [0u8; POR_PAGINA * 4];
        for p in paginas {
            if p >= ENTRADAS / POR_PAGINA {
                continue;
            }
            // Se lee la página una vez; lo que el invitado cambie mientras, va al siguiente vaciado.
            let bien = self.tabla_buena && mem.leer(base + (p * POR_PAGINA * 4) as u64, &mut buf).is_ok();
            for i in 0..POR_PAGINA {
                let n = p * POR_PAGINA + i;
                let e = if bien { u32::from_le_bytes(buf[4 * i..4 * i + 4].try_into().unwrap()) } else { 0 };
                let s = self.traducir(e, t);
                if s != self.sombra[n] {
                    self.sombra[n] = s;
                    hw.escribir_sombra(n, s);
                    cambiadas += 1;
                }
            }
        }
        self.cuentas.cambiadas += cambiadas as u64;
        cambiadas
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::memoria::Ram;
    use alloc::collections::BTreeMap;

    const RAM: u64 = 0x4000_0000;
    const TAM: usize = 16 << 20;
    /// Donde el invitado pone su tabla (4 MB) y su página de relleno.
    const TABLA: u64 = RAM + (8 << 20);
    const RELLENO: u64 = RAM + 0x1000;

    /// La RAM del invitado está en páginas físicas desordenadas: la del invitado `i` es la física 0x9_0000 + (i·7 mod
    /// n). Fuera de la RAM, nada.
    struct Desordenada;
    impl Traductor for Desordenada {
        fn pagina_fisica(&self, p: u64) -> Option<u64> {
            let n = (TAM >> 12) as u64;
            let i = p.checked_sub(RAM >> 12).filter(|&i| i < n)?;
            Some(0x9_0000 + (i * 7) % n)
        }
    }

    #[derive(Default)]
    struct Falsa {
        regs: BTreeMap<(u8, u64), u32>,
        escritas: Vec<(Bloque, u64, u32)>,
        sombra: BTreeMap<usize, u32>,
    }
    impl V3dReal for Falsa {
        fn leer(&mut self, b: Bloque, d: u64) -> u32 {
            self.regs.get(&(b as u8, d)).copied().unwrap_or(0)
        }
        fn escribir(&mut self, b: Bloque, d: u64, v: u32) {
            self.regs.insert((b as u8, d), v);
            self.escritas.push((b, d, v));
        }
        fn escribir_sombra(&mut self, n: usize, v: u32) {
            self.sombra.insert(n, v);
        }
        fn pagina_sombra(&self) -> u32 {
            0x7_0000
        }
        fn pagina_relleno(&self) -> u32 {
            0x6_ffff
        }
    }

    fn pte(ipa: u64) -> u32 {
        PTE_VALID | PTE_WRITEABLE | (ipa >> 12) as u32
    }

    /// Como Linux (`v3d.md`, 5.4 y 5.6): la tabla, la MMU, la página de relleno y el vaciado.
    fn arrancar_como_linux(m: &mut Mediador, hw: &mut Falsa, mem: &Ram<Vec<u8>>) {
        m.escribir(hw, mem, &Desordenada, Bloque::Hub, MMU_PT_PA_BASE, (TABLA >> 12) as u32);
        m.escribir(hw, mem, &Desordenada, Bloque::Hub, MMU_CTL, 0x060d_0c01);
        m.escribir(hw, mem, &Desordenada, Bloque::Hub, MMU_ILLEGAL_ADDR, ILLEGAL_ENABLE | (RELLENO >> 12) as u32);
        m.escribir(hw, mem, &Desordenada, Bloque::Hub, MMUC_CONTROL, MMUC_ENABLE);
        m.escribir(hw, mem, &Desordenada, Bloque::Hub, MMUC_CONTROL, MMUC_ENABLE | MMUC_FLUSH);
        let ctl = m.leer(hw, Bloque::Hub, MMU_CTL);
        m.escribir(hw, mem, &Desordenada, Bloque::Hub, MMU_CTL, ctl | CTL_TLB_CLEAR);
    }

    fn banco() -> (Mediador, Falsa, Ram<Vec<u8>>) {
        let mut m = Mediador::nuevo();
        let mut hw = Falsa::default();
        m.programar(&mut hw);
        (m, hw, Ram::nueva(RAM, TAM))
    }

    #[test]
    fn la_v3d_siempre_ve_la_sombra() {
        let (mut m, mut hw, mut mem) = banco();
        // Tres búferes: VA 0x1000 → RAM + 0x20000, VA 0x2000 → RAM + 0x21000 (sin escribir), VA 0x3000 → última página.
        mem.escribir_u32(TABLA + 4, pte(RAM + 0x20000)).unwrap();
        mem.escribir_u32(TABLA + 8, PTE_VALID | ((RAM + 0x21000) >> 12) as u32).unwrap();
        mem.escribir_u32(TABLA + 12, pte(RAM + TAM as u64 - 4096)).unwrap();
        arrancar_como_linux(&mut m, &mut hw, &mem);
        assert_eq!(hw.leer(Bloque::Hub, MMU_PT_PA_BASE), 0x7_0000, "la sombra, no la del invitado");
        assert_eq!(m.leer(&mut hw, Bloque::Hub, MMU_PT_PA_BASE), (TABLA >> 12) as u32, "el invitado ve la suya");
        assert_eq!(hw.leer(Bloque::Hub, MMU_CTL) & CTL_SEGURO, CTL_SEGURO);
        let n = (TAM >> 12) as u64;
        assert_eq!(m.sombra(1), PTE_VALID | PTE_WRITEABLE | (0x9_0000 + (0x20 * 7) % n) as u32);
        assert_eq!(m.sombra(2), PTE_VALID | (0x9_0000 + (0x21 * 7) % n) as u32, "de solo lectura sigue así");
        assert_eq!(m.sombra(3), PTE_VALID | PTE_WRITEABLE | (0x9_0000 + ((n - 1) * 7) % n) as u32);
        assert_eq!(hw.sombra.get(&1), Some(&m.sombra(1)));
        assert_eq!(m.sombra(0), 0);
        assert_eq!(m.sombra(4), 0);
        // La página de relleno del invitado, traducida.
        assert_eq!(hw.leer(Bloque::Hub, MMU_ILLEGAL_ADDR), ILLEGAL_ENABLE | (0x9_0000 + 7) as u32);
    }

    #[test]
    fn hostil_entradas_fuera_de_su_ram() {
        let (mut m, mut hw, mut mem) = banco();
        // La dirección física 0 (donde está el kernel de DashOS en la Pi), una justo después de su RAM, la página 0xffffff y una
        // con basura en los bits altos.
        mem.escribir_u32(TABLA + 4, PTE_VALID | PTE_WRITEABLE).unwrap();
        mem.escribir_u32(TABLA + 8, pte(RAM + TAM as u64)).unwrap();
        mem.escribir_u32(TABLA + 12, PTE_VALID | PTE_WRITEABLE | 0x00ff_ffff).unwrap();
        mem.escribir_u32(TABLA + 16, 0xffff_ffff).unwrap();
        mem.escribir_u32(TABLA + 20, PTE_SUPERPAGE | PTE_BIGPAGE | 0x0f00_0000 | pte(RAM)).unwrap();
        arrancar_como_linux(&mut m, &mut hw, &mem);
        assert_eq!((m.sombra(1), m.sombra(2), m.sombra(3), m.sombra(4)), (0, 0, 0, 0));
        assert_eq!(m.cuentas.fuera, 8, "4 entradas, en los dos vaciados de Linux");
        // Los bits de página grande y los desconocidos, quitados.
        assert_eq!(m.sombra(5), PTE_VALID | PTE_WRITEABLE | 0x9_0000);
    }

    #[test]
    fn hostil_no_puede_apagar_la_mmu_ni_saltarsela() {
        let (mut m, mut hw, mem) = banco();
        arrancar_como_linux(&mut m, &mut hw, &mem);
        // Apagar la MMU y los abortos, con una excepción puesta: a la V3D le sigue llegando lo seguro.
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMU_CTL, CTL_PT_INVALID_INT | (1 << 9) | (1 << 17) | (1 << 24));
        assert_eq!(hw.leer(Bloque::Hub, MMU_CTL) & (CTL_SEGURO | 1 << 9 | 1 << 17 | 1 << 24), CTL_SEGURO);
        assert_eq!(m.leer(&mut hw, Bloque::Hub, MMU_CTL) & CTL_ENABLE, 0, "él cree que la apagó");
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMUC_CONTROL, 0);
        assert_eq!(hw.leer(Bloque::Hub, MMUC_CONTROL) & MMUC_ENABLE, MMUC_ENABLE);
        // Zonas sin traducir, tope, derribo, y registros que no existen en la página de la MMU.
        let antes = hw.escritas.len();
        for d in [MMU_BYPASS_START, MMU_BYPASS_END, MMU_ADDR_CAP, MMU_SHOOT_DOWN, 0x1100, 0x1ffc, 0x2000, 0x3ffc] {
            m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, d, 0xffff_ffff);
            assert_eq!(m.leer(&mut hw, Bloque::Hub, d), 0, "{d:#x}");
        }
        assert_eq!(hw.escritas.len(), antes, "nada llega a la V3D");
        // Fuera del bloque o sin alinear.
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, 0x4000, 1);
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMU_PT_PA_BASE + 1, 1);
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Nucleo, u64::MAX - 3, 1);
        assert_eq!(hw.escritas.len(), antes);
        // El GMP: solo pedir que pare.
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Nucleo, GMP_CFG, 0xffff_ffff);
        assert_eq!(hw.leer(Bloque::Nucleo, GMP_CFG), GMP_STOP_REQ);
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Nucleo, 0x810, 0x1234);
        assert_eq!(hw.leer(Bloque::Nucleo, 0x810), 0);
        assert!(m.cuentas.prohibidas >= 13);
        // Su página de relleno fuera de su RAM, o apagada: la V3D usa la de DashOS, siempre encendida.
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMU_ILLEGAL_ADDR, ILLEGAL_ENABLE | 0x10);
        assert_eq!(hw.leer(Bloque::Hub, MMU_ILLEGAL_ADDR), ILLEGAL_ENABLE | 0x6_ffff);
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMU_ILLEGAL_ADDR, 0);
        assert_eq!(hw.leer(Bloque::Hub, MMU_ILLEGAL_ADDR), ILLEGAL_ENABLE | 0x6_ffff);
        // Lo demás pasa: una lista de control (sus direcciones son de la MMU).
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Nucleo, 0x16c, 0x1234_5000);
        assert_eq!(hw.leer(Bloque::Nucleo, 0x16c), 0x1234_5000);
    }

    #[test]
    fn hostil_tabla_fuera_de_su_ram() {
        let (mut m, mut hw, mut mem) = banco();
        mem.escribir_u32(TABLA + 4, pte(RAM)).unwrap();
        arrancar_como_linux(&mut m, &mut hw, &mem);
        assert_ne!(m.sombra(1), 0);
        // Una tabla que empieza en su RAM y se sale (los 4 MB no caben), y una que no es suya: la sombra se vacía.
        for base in [(RAM + TAM as u64 - 4096) >> 12, 0, 0xffff_ffff, (RAM >> 12) - 1] {
            m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMU_PT_PA_BASE, base as u32);
            m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMUC_CONTROL, MMUC_ENABLE | MMUC_FLUSH);
            assert_eq!(m.sombra(1), 0, "{base:#x}");
            assert_eq!(hw.leer(Bloque::Hub, MMU_PT_PA_BASE), 0x7_0000);
        }
        assert_eq!(m.cuentas.tablas_malas, 4);
        // Y vuelve a la buena.
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMU_PT_PA_BASE, (TABLA >> 12) as u32);
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMUC_CONTROL, MMUC_ENABLE | MMUC_FLUSH);
        assert_ne!(m.sombra(1), 0);
    }

    #[test]
    fn cambiar_la_tabla_sin_vaciar_no_llega_a_la_v3d() {
        let (mut m, mut hw, mut mem) = banco();
        arrancar_como_linux(&mut m, &mut hw, &mem);
        // El invitado mete una entrada mala después del vaciado: la V3D no la ve; al vaciar, se comprueba.
        mem.escribir_u32(TABLA + 40, PTE_VALID | PTE_WRITEABLE).unwrap();
        mem.escribir_u32(TABLA + 44, pte(RAM + 0x5000)).unwrap();
        assert_eq!((m.sombra(10), m.sombra(11)), (0, 0));
        assert!(!hw.sombra.contains_key(&11));
        let ctl = m.leer(&mut hw, Bloque::Hub, MMU_CTL);
        m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMU_CTL, ctl | CTL_TLB_CLEAR);
        assert_eq!(m.sombra(10), 0);
        assert_ne!(m.sombra(11), 0);
        // Quitarla (Linux escribe 0) también llega, solo al vaciar.
        mem.escribir_u32(TABLA + 44, 0).unwrap();
        assert_ne!(m.sombra(11), 0);
        assert_eq!(m.repasar_paginas(&mut hw, &mem, &Desordenada, [0, 5000, usize::MAX]), 1);
        assert_eq!((m.sombra(11), hw.sombra.get(&11).copied()), (0, Some(0)));
    }

    #[test]
    fn repasar_solo_escribe_lo_que_cambia() {
        let (mut m, mut hw, mut mem) = banco();
        for i in 0..2000u64 {
            mem.escribir_u32(TABLA + 4 * (i + 1), pte(RAM + 0x10_0000 + 4096 * i)).unwrap();
        }
        arrancar_como_linux(&mut m, &mut hw, &mem);
        assert_eq!(m.cuentas.cambiadas, 2000);
        let n = hw.sombra.len();
        assert_eq!(m.repasar(&mut hw, &mem, &Desordenada), 0);
        assert_eq!(hw.sombra.len(), n);
    }

    #[test]
    fn a_lo_loco() {
        let (mut m, mut hw, mut mem) = banco();
        let mut s: u64 = 0x1234_5678_9abc_def1;
        let mut azar = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..3000 {
            let r = azar();
            match r % 6 {
                0 | 1 => {
                    // Casi siempre en la primera página de la tabla, para que los repasos la vean.
                    let n = if r & 0x100 != 0 { azar() % 1024 } else { azar() % ENTRADAS as u64 };
                    let v = if r & 0x200 != 0 { pte(RAM + (azar() % (TAM as u64 + 0x10_0000))) } else { azar() as u32 };
                    let _ = mem.escribir_u32(TABLA + 4 * n, v);
                }
                2 => {
                    let d = 0x1000 + (azar() % 0x300) * 4;
                    m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, d, azar() as u32);
                }
                3 => {
                    m.escribir(&mut hw, &mem, &Desordenada, Bloque::Hub, MMU_PT_PA_BASE, [(TABLA >> 12) as u32, azar() as u32][(r >> 8) as usize % 2]);
                }
                4 => {
                    m.escribir(&mut hw, &mem, &Desordenada, Bloque::Nucleo, (azar() % 0x1000) * 4, azar() as u32);
                }
                _ => {
                    m.repasar_paginas(&mut hw, &mem, &Desordenada, [(azar() % 1100) as usize, 0]);
                }
            }
            // Lo que importa: la V3D nunca tiene la MMU apagada, ni zonas sin traducir, ni una entrada válida fuera de la
            // RAM del invitado.
            assert_eq!(hw.leer(Bloque::Hub, MMU_CTL) & CTL_SEGURO, CTL_SEGURO);
            assert_eq!(hw.leer(Bloque::Hub, MMU_PT_PA_BASE), 0x7_0000);
            assert_eq!(hw.leer(Bloque::Hub, MMU_BYPASS_START) | hw.leer(Bloque::Hub, MMU_BYPASS_END), 0);
        }
        let n = (TAM >> 12) as u64;
        for (&k, &v) in &hw.sombra {
            assert_eq!(v, m.sombra(k));
            if v & PTE_VALID != 0 {
                let p = (v & PTE_PAGINA) as u64;
                assert!((0x9_0000..0x9_0000 + n).contains(&p), "entrada {k}: {v:#x}");
                assert_eq!(v & (PTE_SUPERPAGE | PTE_BIGPAGE | 0x0f00_0000), 0);
            }
        }
    }
}
