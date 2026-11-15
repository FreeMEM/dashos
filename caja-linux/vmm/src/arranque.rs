//! La carga de un kernel de Linux para arm64, según su protocolo de arranque (`Documentation/arch/arm64/booting.rst`,
//! leído como especificación).
//!
//! **La cabecera de la Image** (64 bytes, little-endian): `code0` u32, `code1` u32, `text_offset` u64, `image_size`
//! u64, `flags` u64, tres u64 reservados, la magia `"ARM\x64"` (u32 en 56) y un u32 reservado (el desplazamiento de
//! la cabecera PE en las Image con EFI). `flags`: bit 0 = big-endian, bits 1-2 = página (1: 4K, 2: 16K, 3: 64K),
//! bit 3 = puede ir en cualquier parte de la RAM (si no, lo más cerca posible del principio).
//!
//! **Dónde va cada cosa** ([`planificar`]):
//! - la Image, a `text_offset` de una base alineada a 2 MiB, la primera de la RAM; detrás quedan libres `image_size`
//!   bytes (incluye su BSS);
//! - el DTB, alineado a 8 y de 2 MiB como mucho, solo en su bloque de 2 MiB (el kernel lo mapea con bloques de 2 MiB):
//!   en el último bloque de 2 MiB de la RAM;
//! - el initrd, justo debajo del DTB, alineado a página. Tiene que caber, con la Image, en una ventana de 32 GiB
//!   alineada a 1 GiB.
//!
//! **El estado al saltar** ([`RegistrosArranque`]): x0 = dirección del DTB, x1 = x2 = x3 = 0, PC = principio de la
//! Image, EL1h con DAIF enmascarado, MMU apagada y la Image limpia hasta el punto de coherencia. Lo que pide además
//! el protocolo y le toca al hipervisor, al preparar el vCPU: CNTFRQ_EL0 con la frecuencia del temporizador,
//! CNTVOFF_EL2 igual en todos los vCPU, CNTHCTL_EL2.EL1PCTEN = 1 y los registros de sistema de EL1 con valores
//! conocidos (SCTLR_EL1 con la MMU y la caché de datos apagadas).

use crate::memoria::{FueraDeMemoria, MemoriaInvitado};

pub const MAGIA: u32 = 0x644d_5241; // "ARM\x64"
pub const TAM_CABECERA: usize = 64;
pub const DOS_MIB: u64 = 2 << 20;
pub const UN_GIB: u64 = 1 << 30;
pub const VENTANA_INITRD: u64 = 32 << 30;
pub const PAGINA: u64 = 4096;
/// EL1h (SPSel = 1) con D, A, I y F enmascarados.
pub const PSTATE_ARRANQUE: u64 = 0x3c5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorArranque {
    CabeceraCorta,
    SinMagia,
    /// `image_size` a 0: un kernel anterior a 3.17, sin tamaño conocido de la BSS. No se admite.
    TamañoDesconocido,
    DtbGrande,
    /// No caben la Image, el initrd y el DTB en la RAM dada.
    NoCabe,
    Memoria(FueraDeMemoria),
}

impl From<FueraDeMemoria> for ErrorArranque {
    fn from(e: FueraDeMemoria) -> Self {
        ErrorArranque::Memoria(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CabeceraImage {
    pub text_offset: u64,
    pub image_size: u64,
    pub flags: u64,
}

impl CabeceraImage {
    pub fn big_endian(&self) -> bool {
        self.flags & 1 != 0
    }
    /// Tamaño de página del kernel, si lo dice.
    pub fn pagina(&self) -> Option<u64> {
        match (self.flags >> 1) & 3 {
            1 => Some(4 << 10),
            2 => Some(16 << 10),
            3 => Some(64 << 10),
            _ => None,
        }
    }
    pub fn en_cualquier_sitio(&self) -> bool {
        self.flags & 8 != 0
    }
}

fn u64_en(b: &[u8], o: usize) -> u64 {
    let mut x = [0u8; 8];
    x.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(x)
}

pub fn leer_cabecera(imagen: &[u8]) -> Result<CabeceraImage, ErrorArranque> {
    if imagen.len() < TAM_CABECERA {
        return Err(ErrorArranque::CabeceraCorta);
    }
    let magia = u32::from_le_bytes([imagen[56], imagen[57], imagen[58], imagen[59]]);
    if magia != MAGIA {
        return Err(ErrorArranque::SinMagia);
    }
    let c = CabeceraImage { text_offset: u64_en(imagen, 8), image_size: u64_en(imagen, 16), flags: u64_en(imagen, 24) };
    if c.image_size == 0 {
        return Err(ErrorArranque::TamañoDesconocido);
    }
    Ok(c)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanCarga {
    /// Dirección de la Image (y punto de entrada).
    pub kernel: u64,
    /// Fin de lo que la Image necesita libre (`kernel + image_size`).
    pub fin_kernel: u64,
    pub dtb: u64,
    /// Bytes reservados para el DTB (el bloque de 2 MiB).
    pub dtb_max: u64,
    /// Inicio y fin (exclusivo) del initrd.
    pub initrd: Option<(u64, u64)>,
}

fn alinear_arriba(v: u64, a: u64) -> Option<u64> {
    v.checked_add(a - 1).map(|x| x & !(a - 1))
}

fn alinear_abajo(v: u64, a: u64) -> u64 {
    v & !(a - 1)
}

/// Dónde poner la Image, el DTB y el initrd en una región de RAM `(base, tamaño)`.
pub fn planificar(
    cab: &CabeceraImage,
    largo_imagen: u64,
    ram: (u64, u64),
    largo_initrd: Option<u64>,
) -> Result<PlanCarga, ErrorArranque> {
    let (base, tam) = ram;
    let fin_ram = base.checked_add(tam).ok_or(ErrorArranque::NoCabe)?;
    let base2m = alinear_arriba(base, DOS_MIB).ok_or(ErrorArranque::NoCabe)?;
    let kernel = base2m.checked_add(cab.text_offset).ok_or(ErrorArranque::NoCabe)?;
    let fin_kernel = kernel.checked_add(cab.image_size.max(largo_imagen)).ok_or(ErrorArranque::NoCabe)?;
    // El initrd y el DTB, dentro de la ventana de 32 GiB alineada a 1 GiB que empieza bajo la Image.
    let tope = fin_ram.min(alinear_abajo(kernel, UN_GIB).saturating_add(VENTANA_INITRD));
    let dtb = alinear_abajo(tope, DOS_MIB).checked_sub(DOS_MIB).ok_or(ErrorArranque::NoCabe)?;
    let libre = alinear_arriba(fin_kernel, PAGINA).ok_or(ErrorArranque::NoCabe)?;
    if dtb < libre || dtb < base {
        return Err(ErrorArranque::NoCabe);
    }
    let initrd = match largo_initrd {
        None | Some(0) => None,
        Some(n) => {
            let ini = alinear_abajo(dtb.checked_sub(n).ok_or(ErrorArranque::NoCabe)?, PAGINA);
            if ini < libre {
                return Err(ErrorArranque::NoCabe);
            }
            Some((ini, ini + n))
        }
    };
    Ok(PlanCarga { kernel, fin_kernel, dtb, dtb_max: DOS_MIB, initrd })
}

/// Los registros del vCPU 0 al entrar en el kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistrosArranque {
    pub pc: u64,
    /// x0..x3.
    pub x: [u64; 4],
    pub pstate: u64,
}

pub fn registros(plan: &PlanCarga) -> RegistrosArranque {
    RegistrosArranque { pc: plan.kernel, x: [plan.dtb, 0, 0, 0], pstate: PSTATE_ARRANQUE }
}

/// Copia la Image, el initrd y el DTB a la memoria del invitado según el plan.
pub fn cargar(
    mem: &mut dyn MemoriaInvitado,
    plan: &PlanCarga,
    imagen: &[u8],
    initrd: Option<&[u8]>,
    dtb: &[u8],
) -> Result<(), ErrorArranque> {
    if dtb.len() as u64 > plan.dtb_max {
        return Err(ErrorArranque::DtbGrande);
    }
    mem.escribir(plan.kernel, imagen)?;
    if let (Some((ini, fin)), Some(datos)) = (plan.initrd, initrd) {
        if (fin - ini) != datos.len() as u64 {
            return Err(ErrorArranque::NoCabe);
        }
        mem.escribir(ini, datos)?;
    }
    mem.escribir(plan.dtb, dtb)?;
    Ok(())
}

/// Seis instrucciones que hacen lo que hará el hipervisor al preparar el vCPU: x0 = `dtb`, x1..x3 = 0 y saltar a
/// `entrada`. Sirve para probar la carga en una máquina que empieza a ejecutar en una dirección (QEMU sin -kernel).
pub fn trampolin(dtb: u64, entrada: u64) -> [u8; 40] {
    let ins: [u32; 6] = [
        0x5800_00c0, // ldr x0, #24 (literal en +24)
        0x5800_00e4, // ldr x4, #28 (literal en +32)
        0xaa1f_03e1, // mov x1, xzr
        0xaa1f_03e2, // mov x2, xzr
        0xaa1f_03e3, // mov x3, xzr
        0xd61f_0080, // br x4
    ];
    let mut b = [0u8; 40];
    for (i, w) in ins.iter().enumerate() {
        b[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    b[24..32].copy_from_slice(&dtb.to_le_bytes());
    b[32..40].copy_from_slice(&entrada.to_le_bytes());
    b
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::memoria::Ram;

    fn imagen(text_offset: u64, image_size: u64, flags: u64) -> alloc::vec::Vec<u8> {
        let mut b = alloc::vec![0u8; 4096];
        b[8..16].copy_from_slice(&text_offset.to_le_bytes());
        b[16..24].copy_from_slice(&image_size.to_le_bytes());
        b[24..32].copy_from_slice(&flags.to_le_bytes());
        b[56..60].copy_from_slice(&MAGIA.to_le_bytes());
        b
    }

    #[test]
    fn cabecera() {
        let c = leer_cabecera(&imagen(0, 0x10_0000, 0b1010)).unwrap();
        assert_eq!(c.pagina(), Some(4096));
        assert!(c.en_cualquier_sitio());
        assert!(!c.big_endian());
        assert_eq!(leer_cabecera(&[0; 10]), Err(ErrorArranque::CabeceraCorta));
        assert_eq!(leer_cabecera(&[0; 64]), Err(ErrorArranque::SinMagia));
        assert_eq!(leer_cabecera(&imagen(0, 0, 0)), Err(ErrorArranque::TamañoDesconocido));
    }

    #[test]
    fn plan_y_carga() {
        let img = imagen(0x8_0000, 0x40_0000, 0b1010);
        let c = leer_cabecera(&img).unwrap();
        let ram = (0x4010_0000, 0x400_0000); // base sin alinear a 2 MiB
        let p = planificar(&c, img.len() as u64, ram, Some(10_000)).unwrap();
        assert_eq!(p.kernel, 0x4020_0000 + 0x8_0000);
        assert_eq!(p.dtb % DOS_MIB, 0);
        assert_eq!(p.dtb + DOS_MIB, 0x4400_0000);
        let (ini, fin) = p.initrd.unwrap();
        assert_eq!(fin - ini, 10_000);
        assert_eq!(ini % PAGINA, 0);
        assert!(ini >= p.fin_kernel && fin <= p.dtb);
        let r = registros(&p);
        assert_eq!(r.x, [p.dtb, 0, 0, 0]);
        let mut m = Ram::nueva(ram.0, ram.1 as usize);
        let initrd = alloc::vec![5u8; 10_000];
        cargar(&mut m, &p, &img, Some(&initrd), &[1, 2, 3]).unwrap();
        assert_eq!(m.leer_u32(p.kernel + 56), Ok(MAGIA));
        assert_eq!(m.leer_u16(p.dtb), Ok(0x0201));
        assert_eq!(m.leer_u32(ini + 9996), Ok(0x0505_0505));
        assert_eq!(cargar(&mut m, &p, &img, None, &alloc::vec![0; 3 << 20]), Err(ErrorArranque::DtbGrande));
    }

    #[test]
    fn no_cabe() {
        let img = imagen(0, 0x100_0000, 8);
        let c = leer_cabecera(&img).unwrap();
        assert_eq!(planificar(&c, 4096, (0x4000_0000, 0x100_0000), None), Err(ErrorArranque::NoCabe));
        assert_eq!(planificar(&c, 4096, (0x4000_0000, 0x180_0000), Some(8 << 20)), Err(ErrorArranque::NoCabe));
        assert_eq!(planificar(&c, 4096, (u64::MAX - 10, 0x1000_0000), None), Err(ErrorArranque::NoCabe));
        assert_eq!(planificar(&c, 4096, (0, 0), None), Err(ErrorArranque::NoCabe));
        // Más de 32 GiB de RAM: el initrd queda dentro de la ventana de la Image.
        let p = planificar(&c, 4096, (0x4000_0000, 64 << 30), Some(4096)).unwrap();
        assert!(p.initrd.unwrap().1 <= 0x4000_0000 + (32 << 30));
    }

    #[test]
    fn trampolin_literales() {
        let t = trampolin(0x1122_3344_5566_7788, 0x4008_0000);
        assert_eq!(u64::from_le_bytes(t[24..32].try_into().unwrap()), 0x1122_3344_5566_7788);
        assert_eq!(u64::from_le_bytes(t[32..40].try_into().unwrap()), 0x4008_0000);
    }
}
