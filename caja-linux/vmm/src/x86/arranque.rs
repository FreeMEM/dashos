//! Cargar un bzImage de Linux x86-64 en la RAM del invitado por el protocolo de 32 bits (boot.rst §"32-bit boot
//! protocol"): la parte protegida en `CARGA` (1 MB), la página cero con la cabecera de arranque del propio bzImage, el
//! mapa e820, la línea de órdenes y el initramfs (arriba de la RAM, bajo `initrd_addr_max`). La vCPU entra en
//! `code32_start`, en modo protegido plano (CS 0x10, DS 0x18; los pone el kernel de DashOS) con ESI = la página cero.
//!
//! Y la tabla MP (MultiProcessor Specification 1.4, cap. 4): el puntero flotante en `MP` y la tabla de configuración
//! detrás, con una CPU (APIC 0), el bus ISA 0, un IOAPIC (id 1) y las interrupciones ISA 0 a 15 a sus pines.
//!
//! Todo lo que se lee del bzImage se comprueba: una cabecera corta, sin "HdrS", de un protocolo viejo (< 2.06), o que
//! no cabe, da un error.

use crate::memoria::MemoriaInvitado;
use alloc::vec::Vec;

/// Dónde va la parte protegida del kernel.
pub const CARGA: u64 = 0x10_0000;
/// La página cero (`boot_params`).
pub const CERO: u64 = 0x7000;
/// La línea de órdenes.
pub const LINEA: u64 = 0x2_0000;
/// La tabla MP (el puntero flotante y, detrás, la de configuración), en el área de la BIOS.
pub const MP: u64 = 0xF_0000;
pub const LAPIC: u64 = 0xFEE0_0000;
pub const IOAPIC: u64 = 0xFEC0_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FalloArranque {
    /// No es un bzImage (sin "HdrS" o corto).
    NoEsBzImage,
    /// El protocolo es anterior a 2.06 (sin `cmdline_size` ni `initrd_addr_max` fiables).
    ProtocoloViejo,
    /// El kernel, el initramfs o la línea no caben en la RAM.
    NoCabe,
    /// No se pudo escribir en la RAM del invitado.
    Memoria,
}

/// Lo que queda tras cargar: la entrada (física) y la página cero (para ESI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cargado {
    pub entrada: u64,
    pub cero: u64,
    pub initrd: (u64, u64),
}

fn u16_en(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32_en(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Carga `bz` e `initrd` en una RAM de `ram` bytes desde 0, con la línea `linea`.
pub fn cargar(mem: &mut dyn MemoriaInvitado, ram: u64, bz: &[u8], initrd: &[u8], linea: &str) -> Result<Cargado, FalloArranque> {
    if bz.len() < 0x300 || &bz[0x202..0x206] != b"HdrS" {
        return Err(FalloArranque::NoEsBzImage);
    }
    let version = u16_en(bz, 0x206);
    if version < 0x206 {
        return Err(FalloArranque::ProtocoloViejo);
    }
    let setup = match bz[0x1F1] {
        0 => 4,
        n => n as usize,
    };
    let desde = (setup + 1) * 512;
    if desde >= bz.len() {
        return Err(FalloArranque::NoEsBzImage);
    }
    let protegida = &bz[desde..];
    // `init_size` (2.10): lo que necesita el kernel descomprimido desde la carga.
    let init = if version >= 0x20A { u32_en(bz, 0x260) as u64 } else { protegida.len() as u64 * 4 };
    let max_linea = u32_en(bz, 0x238) as usize;
    let max_initrd = u32_en(bz, 0x22C) as u64;
    if CARGA + init.max(protegida.len() as u64) > ram || linea.len() + 1 > max_linea.min(4096) {
        return Err(FalloArranque::NoCabe);
    }
    // El initramfs, lo más arriba posible (bajo el tope de la RAM y el del kernel), alineado a la página.
    let tope = ram.min(max_initrd + 1);
    let n = initrd.len() as u64;
    let initrd_dir = tope.checked_sub(n).map(|d| d & !0xFFF).filter(|&d| d >= CARGA + init).ok_or(FalloArranque::NoCabe)?;
    let m = |r: Result<(), crate::memoria::FueraDeMemoria>| r.map_err(|_| FalloArranque::Memoria);
    m(mem.escribir(CARGA, protegida))?;
    if !initrd.is_empty() {
        m(mem.escribir(initrd_dir, initrd))?;
    }
    let mut l = Vec::from(linea.as_bytes());
    l.push(0);
    m(mem.escribir(LINEA, &l))?;
    // La página cero: a cero, con la cabecera del bzImage (de 0x1F1 a 0x202 + su largo) en su sitio.
    let mut cero = alloc::vec![0u8; 4096];
    let fin_cab = (0x202 + bz[0x201] as usize).min(bz.len()).min(4096);
    cero[0x1F1..fin_cab].copy_from_slice(&bz[0x1F1..fin_cab]);
    cero[0x210] = 0xFF; // type_of_loader: sin registrar
    cero[0x211] |= 0x01; // LOADED_HIGH
    cero[0x211] &= !0x20; // sin QUIET_FLAG: que hable
    cero[0x218..0x21C].copy_from_slice(&(if initrd.is_empty() { 0 } else { initrd_dir as u32 }).to_le_bytes());
    cero[0x21C..0x220].copy_from_slice(&(n as u32).to_le_bytes());
    cero[0x228..0x22C].copy_from_slice(&(LINEA as u32).to_le_bytes());
    // El e820: memoria baja, el hueco de la BIOS (con la tabla MP) y el resto de la RAM.
    let e820 = [(0u64, 0x9_FC00u64, 1u32), (0x9_FC00, 0x10_0000 - 0x9_FC00, 2), (0x10_0000, ram - 0x10_0000, 1)];
    for (k, (dir, largo, tipo)) in e820.iter().enumerate() {
        let o = 0x2D0 + 20 * k;
        cero[o..o + 8].copy_from_slice(&dir.to_le_bytes());
        cero[o + 8..o + 16].copy_from_slice(&largo.to_le_bytes());
        cero[o + 16..o + 20].copy_from_slice(&tipo.to_le_bytes());
    }
    cero[0x1E8] = e820.len() as u8;
    m(mem.escribir(CERO, &cero))?;
    m(mem.escribir(MP, &tabla_mp()))?;
    Ok(Cargado { entrada: u32_en(bz, 0x214) as u64, cero: CERO, initrd: (initrd_dir, n) })
}

fn suma(b: &[u8]) -> u8 {
    0u8.wrapping_sub(b.iter().fold(0u8, |s, &x| s.wrapping_add(x)))
}

/// El puntero flotante (16 bytes en `MP`) y la tabla de configuración detrás (en `MP + 16`).
pub fn tabla_mp() -> Vec<u8> {
    let mut t: Vec<u8> = Vec::new();
    // Cabecera (44 bytes), rellena luego.
    t.resize(44, 0);
    let mut n = 0u16;
    // Procesador: APIC 0, versión 0x14, activo y el de arranque; firma y rasgos del CPUID (FPU, APIC).
    t.extend_from_slice(&[0, 0, 0x14, 0x03]);
    t.extend_from_slice(&0x0006_0F00u32.to_le_bytes());
    t.extend_from_slice(&((1u32 << 0) | (1 << 9)).to_le_bytes());
    t.extend_from_slice(&[0; 8]);
    n += 1;
    // El bus ISA 0.
    t.extend_from_slice(&[1, 0]);
    t.extend_from_slice(b"ISA   ");
    n += 1;
    // El IOAPIC 1, versión 0x11, activo.
    t.extend_from_slice(&[2, 1, 0x11, 1]);
    t.extend_from_slice(&(IOAPIC as u32).to_le_bytes());
    n += 1;
    // Las 16 de ISA a sus pines (como en un PC: la 0 al pin 2 no, aquí todas a su pin, sin PIT que la use). Por nivel
    // y activas en alto (banderas 0b1101), como las líneas de virtio-mmio; el invitado las ve así.
    for irq in 0..16u8 {
        t.extend_from_slice(&[3, 0, 0x0D, 0, 0, irq, 1, irq]);
        n += 1;
    }
    let largo = t.len() as u16;
    t[0..4].copy_from_slice(b"PCMP");
    t[4..6].copy_from_slice(&largo.to_le_bytes());
    t[6] = 4;
    t[8..16].copy_from_slice(b"DASHOS  ");
    t[16..28].copy_from_slice(b"DOMINIO X86 ");
    t[34..36].copy_from_slice(&n.to_le_bytes());
    t[36..40].copy_from_slice(&(LAPIC as u32).to_le_bytes());
    t[7] = suma(&t);
    let mut p = Vec::with_capacity(16 + t.len());
    p.extend_from_slice(b"_MP_");
    p.extend_from_slice(&((MP + 16) as u32).to_le_bytes());
    p.extend_from_slice(&[1, 4, 0, 0, 0, 0, 0, 0]);
    p[10] = suma(&p);
    p.extend_from_slice(&t);
    p
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::memoria::Ram;

    fn bz(version: u16) -> Vec<u8> {
        let mut b = alloc::vec![0u8; 0x1000 + 512 * 5];
        b[0x1F1] = 4;
        b[0x201] = 0x6A;
        b[0x202..0x206].copy_from_slice(b"HdrS");
        b[0x206..0x208].copy_from_slice(&version.to_le_bytes());
        b[0x214..0x218].copy_from_slice(&0x10_0000u32.to_le_bytes());
        b[0x22C..0x230].copy_from_slice(&0x7FFF_FFFFu32.to_le_bytes());
        b[0x238..0x23C].copy_from_slice(&2048u32.to_le_bytes());
        b[0x260..0x264].copy_from_slice(&(4u32 << 20).to_le_bytes());
        b
    }

    #[test]
    fn carga_y_tabla_mp() {
        let mut ram = Ram::nueva(0, 64 << 20);
        let c = cargar(&mut ram, 64 << 20, &bz(0x20F), &[7; 5000], "console=ttyS0").unwrap();
        assert_eq!(c.entrada, 0x10_0000);
        assert!(c.initrd.0 + c.initrd.1 <= 64 << 20 && c.initrd.0 % 4096 == 0);
        let mut cero = [0u8; 4096];
        ram.leer(CERO, &mut cero).unwrap();
        assert_eq!(&cero[0x202..0x206], b"HdrS");
        assert_eq!(cero[0x1E8], 3);
        let mut mp = [0u8; 16];
        ram.leer(MP, &mut mp).unwrap();
        assert_eq!(&mp[..4], b"_MP_");
        assert_eq!(mp.iter().fold(0u8, |s, &x| s.wrapping_add(x)), 0);
        let t = tabla_mp();
        let largo = u16::from_le_bytes([t[20], t[21]]) as usize;
        assert_eq!(t[16..16 + largo].iter().fold(0u8, |s, &x| s.wrapping_add(x)), 0);
    }

    #[test]
    fn bzimage_hostil() {
        let mut ram = Ram::nueva(0, 64 << 20);
        assert_eq!(cargar(&mut ram, 64 << 20, &[0; 100], &[], ""), Err(FalloArranque::NoEsBzImage));
        assert_eq!(cargar(&mut ram, 64 << 20, &bz(0x205), &[], ""), Err(FalloArranque::ProtocoloViejo));
        assert_eq!(cargar(&mut ram, 4 << 20, &bz(0x20F), &[], ""), Err(FalloArranque::NoCabe));
        assert_eq!(cargar(&mut ram, 64 << 20, &bz(0x20F), &[0; 70 << 20], ""), Err(FalloArranque::NoCabe));
        assert_eq!(cargar(&mut ram, 64 << 20, &bz(0x20F), &[], &"x".repeat(5000)), Err(FalloArranque::NoCabe));
        let mut b = bz(0x20F);
        b[0x1F1] = 200;
        assert_eq!(cargar(&mut ram, 64 << 20, &b, &[], ""), Err(FalloArranque::NoEsBzImage));
    }
}
