//! GPU V3D 4.2 de la BCM2711, lado del kernel (C24).
//!
//! Escrito solo con `docs/spec/hw/v3d.md` (sala limpia). Las referencias "§" son a sus secciones.
//!
//! Lo que hay: encender el dominio y el reloj, leer la identificación, la MMU de la GPU (una tabla de 4 MB para
//! todos), el estado fijo del núcleo, las interrupciones, la invalidación y limpieza de cachés, y lanzar binning y
//! render. Las listas de control las escribirá Mesa portada (V4, `docs/spec/hw/mesa-v3d.md`).
//!
//! Sin probar: QEMU `raspi4b` no emula la V3D. En QEMU solo se comprueba la codificación (entradas de la tabla en
//! little-endian, valores de registro de la especificación) y que el controlador ve que no hay GPU. Falta la Pi 4.

#![allow(dead_code)] // los trabajos los pedirá el servidor de la GPU, que aún no existe

use crate::memoria::{paginas, Zona, PAGINA};
use crate::mmio::RegLe32;
use crate::{buzon, mmu, reloj};

// Ventanas físicas (§2.1, §2.4), con el modo de periféricos bajo.
const HUB: usize = 0xFEC0_0000;
const NUCLEO: usize = 0xFEC0_4000;
const PM: usize = 0xFE10_0000;
const ASB_RPIVID: usize = 0xFEC1_1000;
const CPRMAN: usize = 0xFE10_1000;

// Bloque PM, ASB y cprman (§2.4). Toda escritura lleva la contraseña.
const CONTRASENA: u32 = 0x5A00_0000;
const PM_GRAFX: usize = 0x10C;
const PM_GRAFX_V3DRSTN: u32 = 1 << 6;
const ASB_S_CTRL: usize = 0x08;
const ASB_M_CTRL: usize = 0x0C;
const ASB_REQ_STOP: u32 = 1 << 0;
const ASB_ACK: u32 = 1 << 1;
const ASB_BRDG_ID: usize = 0x20;
const ASB_ID: u32 = 0x6272_6467; // "brdg"
const CM_V3DCTL: usize = 0x038;
const CM_ENABLE: u32 = 1 << 4;
const CM_GATE: u32 = 1 << 6;
const CM_BUSY: u32 = 1 << 7;

// Reloj del firmware (§2.3).
const RELOJ_V3D: u32 = 5;

// Hub (§4.1).
const HUB_UIFCFG: usize = 0x004;
const HUB_IDENT1: usize = 0x00C;
const HUB_IDENT2: usize = 0x010;
const HUB_IDENT3: usize = 0x014;
const INT_STS: usize = 0x050;
const INT_CLR: usize = 0x058;
const INT_MSK_SET: usize = 0x060;
const INT_MSK_CLR: usize = 0x064;
const MMUC_CONTROL: usize = 0x1000;
const MMU_CTL: usize = 0x1200;
const MMU_PT_PA_BASE: usize = 0x1204;
const MMU_ILLEGAL_ADDR: usize = 0x1230;
const MMU_DEBUG_INFO: usize = 0x1238;

// Núcleo (§4.2).
const CTL_IDENT0: usize = 0x000;
const CTL_IDENT1: usize = 0x004;
const CTL_IDENT2: usize = 0x008;
const CTL_SLCACTL: usize = 0x024;
const CTL_L2TCACTL: usize = 0x030;
const CTL_L2TFLSTA: usize = 0x034;
const CTL_L2TFLEND: usize = 0x038;
const CLE_CT0QTS: usize = 0x15C;
const CLE_CT0QBA: usize = 0x160;
const CLE_CT1QBA: usize = 0x164;
const CLE_CT0QEA: usize = 0x168;
const CLE_CT1QEA: usize = 0x16C;
const CLE_CT0QMA: usize = 0x170;
const CLE_CT0QMS: usize = 0x174;
const PTB_BPOS: usize = 0x30C;

// MMU (§5.3).
const MMU_ENABLE: u32 = 1 << 0;
const MMU_TLB_CLEAR: u32 = 1 << 2;
const MMU_TLB_CLEARING: u32 = 1 << 7;
const MMU_WRITE_VIOLATION_INT: u32 = 1 << 10;
const MMU_WRITE_VIOLATION_ABORT: u32 = 1 << 11;
const MMU_PT_INVALID_ENABLE: u32 = 1 << 16;
const MMU_PT_INVALID_INT: u32 = 1 << 18;
const MMU_PT_INVALID_ABORT: u32 = 1 << 19;
const MMU_CAP_EXCEEDED_INT: u32 = 1 << 25;
const MMU_CAP_EXCEEDED_ABORT: u32 = 1 << 26;
const MMUC_ENABLE: u32 = 1 << 0;
const MMUC_FLUSH: u32 = 1 << 1;
const MMUC_FLUSHING: u32 = 1 << 2;

/// Valor de `MMU_CTL` al dar la tabla (§5.4).
pub const MMU_CTL_ARRANQUE: u32 = MMU_ENABLE | MMU_PT_INVALID_ENABLE | MMU_PT_INVALID_ABORT | MMU_PT_INVALID_INT
    | MMU_WRITE_VIOLATION_ABORT | MMU_WRITE_VIOLATION_INT | MMU_CAP_EXCEEDED_ABORT | MMU_CAP_EXCEEDED_INT;

// Entradas de la tabla (§5.2).
const PTE_WRITEABLE: u32 = 1 << 29;
const PTE_VALID: u32 = 1 << 28;

// Interrupciones (§9.2, §9.3).
pub const INT_FRDONE: u32 = 1 << 0;
pub const INT_FLDONE: u32 = 1 << 1;
pub const INT_OUTOMEM: u32 = 1 << 2;
pub const INT_GMPV: u32 = 1 << 5;
pub const INT_CSDDONE: u32 = 1 << 7;
pub const MASCARA_NUCLEO: u32 = INT_FRDONE | INT_FLDONE | INT_OUTOMEM | INT_GMPV | INT_CSDDONE;
const HUB_TFUC: u32 = 1 << 1;
const HUB_MMU_CAP: u32 = 1 << 3;
const HUB_MMU_PTI: u32 = 1 << 4;
const HUB_MMU_WRV: u32 = 1 << 5;
pub const MASCARA_HUB: u32 = HUB_TFUC | HUB_MMU_CAP | HUB_MMU_PTI | HUB_MMU_WRV;

/// La tabla de páginas cubre 4 GB de VA con páginas de 4 KB: 1 048 576 entradas, 4 MB contiguos (§5.1).
const ENTRADAS: usize = 1 << 20;
const PAGINAS_TABLA: usize = ENTRADAS * 4 / PAGINA as usize;

fn r(base: usize, off: usize) -> RegLe32 {
    RegLe32::new(mmu::virt(base) + off)
}

fn esperar_us(us: u64) {
    let fin = reloj::ahora() + us * reloj::frecuencia() / 1_000_000;
    while reloj::ahora() < fin {}
}

/// Espera a que `cond` se cumpla, como mucho `us` microsegundos.
fn esperar_hasta(us: u64, cond: impl Fn() -> bool) -> bool {
    let fin = reloj::ahora() + us * reloj::frecuencia() / 1_000_000;
    while reloj::ahora() < fin {
        if cond() {
            return true;
        }
    }
    cond()
}

/// Entrada de la tabla para una dirección física, ya en el orden de bytes de la GPU (little-endian, §13.2).
pub fn entrada(fisica: u64) -> u32 {
    (PTE_VALID | PTE_WRITEABLE | ((fisica >> 12) as u32 & 0x00FF_FFFF)).to_le()
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    NoHayGpu,
    Puente,
    SinMemoria,
    Plazo,
    Version(u32),
}

/// Identificación (§3).
#[derive(Debug, Clone, Copy)]
pub struct Identidad {
    pub version: u32,
    pub revision: u32,
    pub nucleos: u32,
    pub slices: u32,
    pub qpus: u32,
    pub tmus: u32,
    pub va_bits: u32,
    pub pa_bits: u32,
    /// Los siete registros que Mesa pide tal cual: `HUB_UIFCFG`, `HUB_IDENT1..3`, `CTL_IDENT0..2`.
    pub para_mesa: [u32; 7],
}

/// La MMU de la GPU: una tabla para todos (§5.1).
pub struct Mmu {
    pub tabla: u64,
    pub relleno: u64,
}

impl Mmu {
    pub fn nueva() -> Result<Self, Error> {
        let tabla = paginas().reservar(Zona::Normal, PAGINAS_TABLA, 1).ok_or(Error::SinMemoria)?;
        let relleno = paginas().pagina(Zona::Normal).ok_or(Error::SinMemoria)?;
        // A cero y fuera de la caché: la GPU no ve las cachés de la CPU (§2.7).
        for (p, n) in [(tabla, PAGINAS_TABLA), (relleno, 1)] {
            let va = mmu::virt(p as usize);
            unsafe { core::ptr::write_bytes(va as *mut u8, 0, n * PAGINA as usize) };
            crate::hvs::limpiar_cache(va, n * PAGINA as usize);
        }
        Ok(Self { tabla, relleno })
    }

    /// Mapea páginas físicas desde la VA `va` (múltiplo de 4 KB, nunca la página 0, §6). Hay que vaciar después.
    pub fn mapear(&self, va: u32, fisicas: &[u64]) {
        let base = mmu::virt(self.tabla as usize) as *mut u32;
        let primera = (va >> 12) as usize;
        for (i, &f) in fisicas.iter().enumerate() {
            unsafe { base.add(primera + i).write_volatile(entrada(f)) };
        }
        crate::hvs::limpiar_cache(base as usize + primera * 4, fisicas.len() * 4);
    }

    /// La entrada de una VA, tal como la lee la GPU (little-endian).
    pub fn ver(&self, va: u32) -> u32 {
        unsafe { (mmu::virt(self.tabla as usize) as *const u32).add((va >> 12) as usize).read_volatile() }
    }

    pub fn quitar(&self, va: u32, n: usize) {
        let base = mmu::virt(self.tabla as usize) as *mut u32;
        let primera = (va >> 12) as usize;
        for i in 0..n {
            unsafe { base.add(primera + i).write_volatile(0) };
        }
        crate::hvs::limpiar_cache(base as usize + primera * 4, n * 4);
    }
}

impl Drop for Mmu {
    fn drop(&mut self) {
        paginas().liberar(self.tabla, PAGINAS_TABLA);
        paginas().liberar(self.relleno, 1);
    }
}

pub struct V3d {
    pub identidad: Identidad,
    pub mmu: Mmu,
}

/// ¿Está el puente de la V3D? En QEMU la ventana no existe: se sondea para no caer (C23).
pub fn presente() -> bool {
    r(ASB_RPIVID, ASB_BRDG_ID).sondear() == Some(ASB_ID)
}

/// Activa un lado del puente ASB (§2.4): quitar `REQ_STOP` y esperar a que `ACK` sea 0.
fn puente_activar(off: usize) -> Result<(), Error> {
    let reg = r(ASB_RPIVID, off);
    reg.escribir(CONTRASENA | (reg.leer() & !ASB_REQ_STOP & 0x00FF_FFFF));
    if esperar_hasta(100, || reg.leer() & ASB_ACK == 0) { Ok(()) } else { Err(Error::Puente) }
}

fn reloj_cprman(encender: bool) {
    let cm = r(CPRMAN, CM_V3DCTL);
    if encender {
        cm.escribir(CONTRASENA | (cm.leer() & 0x00FF_FFFF) | CM_ENABLE | CM_GATE);
    } else {
        cm.escribir(CONTRASENA | (cm.leer() & 0x00FF_FFFF & !CM_ENABLE));
        esperar_hasta(1000, || cm.leer() & CM_BUSY == 0);
    }
}

/// Enciende el dominio de la V3D: reloj, reinicio y puente (§2.4).
fn encender_dominio() -> Result<(), Error> {
    reloj_cprman(true);
    esperar_us(1);
    reloj_cprman(false);
    let g = r(PM, PM_GRAFX);
    g.escribir(CONTRASENA | (g.leer() & 0x00FF_FFFF) | PM_GRAFX_V3DRSTN);
    reloj_cprman(true);
    puente_activar(ASB_M_CTRL)?;
    puente_activar(ASB_S_CTRL)
}

/// Enciende el reloj del firmware y lo pone a su máximo (§2.3). Devuelve los Hz.
fn encender_reloj() -> Option<u32> {
    // Mensaje completo: tamaño, petición, etiqueta, tamaño del valor, código, valores y la etiqueta de fin.
    let mut m = [8 * 4, 0, 0x0003_0004, 8, 0, RELOJ_V3D, 0, 0]; // GET_MAX_CLOCK_RATE
    buzon::propiedades(&mut m)?;
    let maximo = m[6];
    let mut e = [8 * 4, 0, 0x0003_8001, 8, 0, RELOJ_V3D, 1, 0]; // SET_CLOCK_STATE, encendido
    buzon::propiedades(&mut e)?;
    let mut f = [9 * 4, 0, 0x0003_8002, 12, 0, RELOJ_V3D, maximo, 0, 0]; // SET_CLOCK_RATE, sin turbo
    buzon::propiedades(&mut f)?;
    Some(f[6])
}

fn leer_identidad() -> Identidad {
    let ident1 = r(HUB, HUB_IDENT1).leer();
    let ident3 = r(HUB, HUB_IDENT3).leer();
    let core1 = r(NUCLEO, CTL_IDENT1).leer();
    let debug = r(HUB, MMU_DEBUG_INFO).leer();
    let slices = (core1 >> 4) & 0xf;
    Identidad {
        version: (ident1 & 0xf) * 10 + ((ident1 >> 4) & 0xf),
        revision: (ident3 >> 8) & 0xff,
        nucleos: (ident1 >> 8) & 0xf,
        slices,
        qpus: slices * ((core1 >> 8) & 0xf),
        tmus: slices * ((core1 >> 12) & 0xf),
        va_bits: 30 + ((debug >> 4) & 0xf),
        pa_bits: 30 + ((debug >> 8) & 0xf),
        para_mesa: [
            r(HUB, HUB_UIFCFG).leer(),
            ident1,
            r(HUB, HUB_IDENT2).leer(),
            ident3,
            r(NUCLEO, CTL_IDENT0).leer(),
            core1,
            r(NUCLEO, CTL_IDENT2).leer(),
        ],
    }
}

impl V3d {
    /// Enciende y prepara la GPU (§2.5).
    pub fn iniciar() -> Result<Self, Error> {
        if !presente() {
            return Err(Error::NoHayGpu);
        }
        let mmu = Mmu::nueva()?;
        encender_dominio()?;
        encender_reloj();
        let identidad = leer_identidad();
        if identidad.version != 42 {
            return Err(Error::Version(identidad.version));
        }
        let g = Self { identidad, mmu };
        g.estado_fijo();
        g.dar_tabla()?;
        g.activar_interrupciones();
        Ok(g)
    }

    fn hub(&self, off: usize) -> RegLe32 {
        r(HUB, off)
    }
    fn nucleo(&self, off: usize) -> RegLe32 {
        r(NUCLEO, off)
    }

    /// §8.3.
    fn estado_fijo(&self) {
        self.nucleo(CTL_L2TFLSTA).escribir(0);
        self.nucleo(CTL_L2TFLEND).escribir(0xFFFF_FFFF);
    }

    /// §5.4.
    fn dar_tabla(&self) -> Result<(), Error> {
        self.hub(MMU_PT_PA_BASE).escribir((self.mmu.tabla >> 12) as u32);
        self.hub(MMU_CTL).escribir(MMU_CTL_ARRANQUE);
        self.hub(MMU_ILLEGAL_ADDR).escribir((self.mmu.relleno >> 12) as u32 | 1 << 31);
        self.hub(MMUC_CONTROL).escribir(MMUC_ENABLE);
        self.vaciar_mmu()
    }

    /// Vacía la caché de la MMU y la TLB (§5.6). Después de cambiar la tabla.
    pub fn vaciar_mmu(&self) -> Result<(), Error> {
        self.hub(MMUC_CONTROL).escribir(MMUC_FLUSH | MMUC_ENABLE);
        if !esperar_hasta(100_000, || self.hub(MMUC_CONTROL).leer() & MMUC_FLUSHING == 0) {
            return Err(Error::Plazo);
        }
        let c = self.hub(MMU_CTL);
        c.escribir(c.leer() | MMU_TLB_CLEAR);
        if esperar_hasta(100_000, || c.leer() & MMU_TLB_CLEARING == 0) { Ok(()) } else { Err(Error::Plazo) }
    }

    /// §9.4.
    fn activar_interrupciones(&self) {
        self.nucleo(INT_CLR).escribir(MASCARA_NUCLEO);
        self.hub(INT_CLR).escribir(MASCARA_HUB);
        self.nucleo(INT_MSK_SET).escribir(!MASCARA_NUCLEO);
        self.nucleo(INT_MSK_CLR).escribir(MASCARA_NUCLEO);
        self.hub(INT_MSK_SET).escribir(!MASCARA_HUB);
        self.hub(INT_MSK_CLR).escribir(MASCARA_HUB);
    }

    /// Lee y borra los eventos del núcleo y del hub (§9.5, mirando siempre los dos).
    pub fn eventos(&self) -> (u32, u32) {
        let n = self.nucleo(INT_STS).leer();
        self.nucleo(INT_CLR).escribir(n);
        let h = self.hub(INT_STS).leer();
        self.hub(INT_CLR).escribir(h);
        (n, h)
    }

    /// Invalida las cachés antes de un trabajo, de fuera hacia dentro (§8.2).
    pub fn invalidar_caches(&self) {
        self.nucleo(CTL_L2TCACTL).escribir(1); // L2TFLS con modo FLUSH; no se espera
        self.nucleo(CTL_SLCACTL).escribir(0x0F0F_0F0F);
    }

    /// Lanza un binning (§7.2). Todo son VA de la GPU. `bcl_fin` es el primer byte tras la lista.
    pub fn lanzar_binning(&self, bcl_inicio: u32, bcl_fin: u32, qma: u32, qms: u32, qts: u32) {
        self.nucleo(PTB_BPOS).escribir(0);
        self.invalidar_caches();
        if qma != 0 {
            self.nucleo(CLE_CT0QMA).escribir(qma);
            self.nucleo(CLE_CT0QMS).escribir(qms);
        }
        if qts != 0 {
            self.nucleo(CLE_CT0QTS).escribir(qts | 1 << 1);
        }
        self.nucleo(CLE_CT0QBA).escribir(bcl_inicio);
        self.nucleo(CLE_CT0QEA).escribir(bcl_fin); // esta escritura lo lanza
    }

    /// Lanza un render (§7.4).
    pub fn lanzar_render(&self, rcl_inicio: u32, rcl_fin: u32) {
        self.invalidar_caches();
        self.nucleo(CLE_CT1QBA).escribir(rcl_inicio);
        self.nucleo(CLE_CT1QEA).escribir(rcl_fin); // esta escritura lo lanza
    }

    /// Espera por consulta a que llegue alguno de los eventos del núcleo `bits`, como mucho `us` microsegundos.
    /// Provisional: con la interrupción en el GIC (INTID 106, §2.2) lo hará el repartidor de C20.
    pub fn esperar_evento(&self, bits: u32, us: u64) -> Option<u32> {
        let fin = reloj::ahora() + us * reloj::frecuencia() / 1_000_000;
        let mut vistos = 0;
        while reloj::ahora() < fin {
            vistos |= self.eventos().0;
            if vistos & bits != 0 {
                return Some(vistos);
            }
        }
        None
    }
}

/// Listas de control mínimas: borrar una baldosa a un color y guardarla en memoria (C27).
///
/// Escrito solo con `docs/spec/hw/v3d-listas.md` (V5), variante A (solo render). De momento solo una baldosa (hasta
/// 64 × 64 píxeles), formato `rgba8` lineal. Los paquetes van byte a byte, con los campos en little-endian (§2).
pub mod listas {
    use alloc::vec::Vec;

    /// Formato de guardado `rgba8` (§3.12).
    const FORMATO_RGBA8: u128 = 27;

    pub struct Borrado {
        pub ancho: u32,
        pub alto: u32,
        /// Palabra del color como la define §3.9: R en los bits bajos (`0xAABBGGRR`).
        pub color: u32,
        pub destino: u32,
        pub paso: u32,
        /// VA de la lista genérica de baldosa.
        pub generica: u32,
        /// VA de una página a cero, alineada a 64 bytes (§3.7).
        pub memoria_baldosas: u32,
    }

    fn mas(v: &mut Vec<u8>, b: &[u8]) {
        v.extend_from_slice(b);
    }

    fn le32(x: u32) -> [u8; 4] {
        x.to_le_bytes()
    }

    /// Guardado de un búfer de baldosa (`STORE_TILE_BUFFER_GENERAL`, 13 bytes).
    fn guardar(v: &mut Vec<u8>, bufer: u8, paso: u32, direccion: u32) {
        let campos: u128 = 0x1D | ((bufer as u128) << 8) | if bufer == 8 { 0 } else { (FORMATO_RGBA8 << 20) | ((paso as u128) << 36) };
        mas(v, &campos.to_le_bytes()[..9]);
        mas(v, &le32(direccion));
    }

    impl Borrado {
        /// La lista genérica de baldosa (§5.2): 17 bytes.
        pub fn lista_generica(&self) -> Vec<u8> {
            let mut v = Vec::new();
            mas(&mut v, &[0x7D, 0x1A]); // TILE_COORDINATES_IMPLICIT, END_OF_LOADS
            guardar(&mut v, 0, self.paso, self.destino);
            mas(&mut v, &[0x1B, 0x12]); // END_OF_TILE_MARKER, RETURN_FROM_SUB_LIST
            v
        }

        /// La RCL (§5.1). `None` si no cabe en una baldosa.
        pub fn rcl(&self) -> Option<Vec<u8>> {
            if self.ancho == 0 || self.alto == 0 || self.ancho > 64 || self.alto > 64 || self.memoria_baldosas % 64 != 0 {
                return None;
            }
            let fin_generica = self.generica + self.lista_generica().len() as u32;
            let (w, h) = (self.ancho as u16, self.alto as u16);
            let mut v = Vec::new();
            // COMMON: un RT, anchura, altura, early-Z desactivado (§3.2, §8.2).
            mas(&mut v, &[0x79, 0x00]);
            mas(&mut v, &w.to_le_bytes());
            mas(&mut v, &h.to_le_bytes());
            mas(&mut v, &[0x40, 0, 0]);
            // CLEAR_COLORS_PART1 del RT 0 (§3.3).
            mas(&mut v, &[0x79, 0x03]);
            mas(&mut v, &le32(self.color));
            mas(&mut v, &[0, 0, 0]);
            // COLOR: 32 bpp, tipo interno rgba8 (§3.4).
            mas(&mut v, &[0x79, 0x81, 0, 0, 0, 0, 0, 0, 0]);
            // ZS_CLEAR_VALUES: plantilla 0, Z 1.0 (§3.5).
            mas(&mut v, &[0x79, 0x02, 0]);
            mas(&mut v, &1.0f32.to_bits().to_le_bytes());
            mas(&mut v, &[0, 0]);
            mas(&mut v, &[0x7E, 0x05]); // TILE_LIST_INITIAL_BLOCK_SIZE
            v.push(0x7B); // MULTICORE_RENDERING_TILE_LIST_SET_BASE, conjunto 0
            mas(&mut v, &le32(self.memoria_baldosas));
            // SUPERTILE_CFG: supertiles de 1 × 1, cuadro de 1 × 1, una baldosa de ancho y de alto (§3.8).
            mas(&mut v, &[0x7A, 0, 0, 1, 1, 1, 0x10, 0, 0]);
            // Dos baldosas de preparación (errata GFXH-1742); la primera borra el TLB.
            for borrar in [true, false] {
                mas(&mut v, &[0x7C, 0, 0, 0, 0x1A]); // TILE_COORDINATES (0, 0), END_OF_LOADS
                guardar(&mut v, 8, 0, 0); // guardado de relleno: ningún búfer
                if borrar {
                    mas(&mut v, &[0x19, 0x03]); // CLEAR_TILE_BUFFERS
                }
                v.push(0x1B); // END_OF_TILE_MARKER
            }
            v.push(0x13); // FLUSH_VCD_CACHE
            v.push(0x14); // START_ADDRESS_OF_GENERIC_TILE_LIST
            mas(&mut v, &le32(self.generica));
            mas(&mut v, &le32(fin_generica));
            mas(&mut v, &[0x17, 0, 0, 0x0D]); // SUPERTILE_COORDINATES (0, 0), END_OF_RENDERING
            Some(v)
        }
    }
}
