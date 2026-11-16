// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! El controlador de la Realtek RTL8125 (X5c, red con cable de 2,5 Gb/s; la de shiva, PCI 10ec:8125). Solo la versión
//! RTL8125B (XID 0x641); las demás se reconocen y se dejan sin tocar. Escrito con la especificación de sala limpia
//! `docs/spec/hw/rtl8125.md` (las secciones se citan como "§"). Nada de Linux, BSD ni QEMU.
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 2); la 3, su espacio de configuración del PCIe; la 4, el
//! firmware del PHY (`rtl8125b-2.fw`: una memoria con su tamaño en la primera palabra y el fichero detrás; tamaño 0 si
//! el sistema no lo tiene); la 5, su interrupción (MSI-X entrada 0), si la hay; la siguiente, el canal "red" (el
//! servicio). Sin interrupción, mira los anillos cada 2 ms.
//!
//! Hacia el servicio, lo mismo que `virtio-red` (§1): `TARJETA` con la MAC, la memoria de las tramas y un canal suyo;
//! `TRAMAS` cuando mueve algo. Una cola de recibir de 128 búferes de 2 KB y una de enviar de 64, con los descriptores
//! de 16 bytes (§7). Sin sumas de comprobación por el aparato, sin TSO, sin VLAN, sin control de flujo, sin EEE, toda la
//! multidifusión (filtra el servicio). Copia cada trama entre la DMA y las ranuras; rellena con ceros hasta 60 bytes al
//! enviar y quita los 4 del CRC al recibir. Si falta el firmware, avisa y sigue (§6.2). Si el servicio se va, la parada
//! ordenada (§12); con un error del bus (`SYSErr`), parada y arranque en marcha con la misma DMA (§11).
//!
//! **Sin probar en hardware**: QEMU no emula esta tarjeta. La prueba es la de verdad, pasando la de shiva a QEMU con
//! VFIO (`make -C kernel arranque-x86 RED_X86=vfio`, `tools/vfio_red.sh`).

#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use dashos::println;
use dashos::protocolo::red as p;
use dashos::rtl8125_fw::{self as fw, Bus};

dashos::programa!(principal);

const VA: usize = 0x10_0000_0000;
const VA_CFG: usize = 0x10_1000_0000;
const VA_FW: usize = 0x10_2000_0000;
const VA_DMA: usize = 0x30_0000_0000;
const VA_TRAMAS: usize = 0x40_0000_0000;

// La memoria DMA: los dos anillos (alineados a 256 bytes, §7.1) y los búferes.
const M_RX: usize = 0x0000;
const M_TX: usize = 0x0800;
const M_RX_BUF: usize = 0x1000;
const BUFER: usize = 2048;
const N_RX: usize = 128;
const N_TX: usize = 64;
const M_TX_BUF: usize = M_RX_BUF + N_RX * BUFER;
const TAM_DMA: usize = M_TX_BUF + N_TX * BUFER;

// Las ranuras compartidas con el servicio.
const RANURAS_RX: usize = 128;
const RANURAS_TX: usize = 64;
const TAM_TRAMAS: usize = 64 + (RANURAS_RX + RANURAS_TX) * p::RANURA;

// Registros (§4).
const MAC0: usize = 0x00;
const MAC4: usize = 0x04;
const MAR0: usize = 0x08;
const MAR4: usize = 0x0C;
const TX_DESC_BAJO: usize = 0x20;
const TX_DESC_ALTO: usize = 0x24;
const INT_CFG0: usize = 0x34;
const CHIP_CMD: usize = 0x37;
const INTR_MASK: usize = 0x38;
const INTR_STATUS: usize = 0x3C;
const TX_CONFIG: usize = 0x40;
const RX_CONFIG: usize = 0x44;
const CFG9346: usize = 0x50;
const CONFIG1: usize = 0x52;
const CONFIG2: usize = 0x53;
const CONFIG3: usize = 0x54;
const CONFIG5: usize = 0x56;
const PHY_STATUS: usize = 0x6C;
const PMCH: usize = 0x6F;
const INT_CFG1: usize = 0x7A;
const EPHYAR: usize = 0x80;
const TX_POLL: usize = 0x90;
const OCPDR: usize = 0xB0;
const GPHY_OCP: usize = 0xB8;
const MCU: usize = 0xD3;
const RX_MAX_SIZE: usize = 0xDA;
const CPLUS_CMD: usize = 0xE0;
const INTR_MITIGATE: usize = 0xE2;
const RX_DESC_BAJO: usize = 0xE4;
const RX_DESC_ALTO: usize = 0xE8;
const MISC: usize = 0xF0;
const MAC0_BKP: usize = 0x19E0;

// Bits.
const CMD_STOP_REQ: u8 = 1 << 7;
const CMD_RESET: u8 = 1 << 4;
const CMD_RX: u8 = 1 << 3;
const CMD_TX: u8 = 1 << 2;
const MCU_NOW_IS_OOB: u8 = 1 << 7;
const MCU_TX_EMPTY: u8 = 1 << 5;
const MCU_RX_EMPTY: u8 = 1 << 4;
const MCU_LINK_LIST_RDY: u8 = 1 << 1;
const MISC_RXDV_GATED: u32 = 1 << 19;
const RXCFG_BASE: u32 = 0x4000_0F00;
/// Unidifusión propia, multidifusión y difusión (§10).
const RXCFG_ACEPTAR: u32 = (1 << 1) | (1 << 2) | (1 << 3);
const TXCFG: u32 = 0x0300_0700;
const I_SYSERR: u32 = 1 << 15;
const I_LINKCHG: u32 = 1 << 5;
/// `RxOK | RxErr | TxOK | TxErr | RxOverflow | LinkChg` (§11).
const MASCARA: u32 = 0x003F;

// Descriptores (§7.1).
const OWN: u32 = 1 << 31;
const RING_END: u32 = 1 << 30;
const FIRST: u32 = 1 << 29;
const LAST: u32 = 1 << 28;
const RX_RES: u32 = 1 << 21;
const RX_RWT: u32 = 1 << 22;
const RX_RUNT: u32 = 1 << 20;
const TX_ERROR: u32 = 1 << 23;

// El PHY (§3.2, §6).
const BMCR: u16 = 0xA400;
const PHYID1: u16 = 0xA404;
const PHYID2: u16 = 0xA406;
const ANAR: u16 = 0xA408;
const GBCR: u16 = 0xA412;
const PHY_ESTADO: u16 = 0xA420;
const PARAM_G_NUM: u16 = 0xA436;
const PARAM_G_VAL: u16 = 0xA438;
const PARAM_8125_NUM: u16 = 0xB87C;
const PARAM_8125_VAL: u16 = 0xB87E;

/// Marcas de lo que llega por el canal propio.
const M_IRQ: u64 = 1;
const M_SERVICIO: u64 = 2;

fn r8(off: usize) -> u8 {
    unsafe { read_volatile((VA + off) as *const u8) }
}
fn w8(off: usize, v: u8) {
    unsafe { write_volatile((VA + off) as *mut u8, v) }
}
fn r16(off: usize) -> u16 {
    u16::from_le(unsafe { read_volatile((VA + off) as *const u16) })
}
fn w16(off: usize, v: u16) {
    unsafe { write_volatile((VA + off) as *mut u16, v.to_le()) }
}
fn r32(off: usize) -> u32 {
    u32::from_le(unsafe { read_volatile((VA + off) as *const u32) })
}
fn w32(off: usize, v: u32) {
    unsafe { write_volatile((VA + off) as *mut u32, v.to_le()) }
}
fn activar8(off: usize, bits: u8) {
    w8(off, r8(off) | bits);
}
fn quitar8(off: usize, bits: u8) {
    w8(off, r8(off) & !bits);
}
fn activar32(off: usize, bits: u32) {
    w32(off, r32(off) | bits);
}
fn quitar32(off: usize, bits: u32) {
    w32(off, r32(off) & !bits);
}
/// Confirmar las escrituras de antes leyendo `ChipCmd` (§3.5).
fn confirmar() {
    r8(CHIP_CMD);
}
fn desbloquear() {
    w8(CFG9346, 0xC0);
}
fn bloquear() {
    w8(CFG9346, 0x00);
}
fn cfg16(off: usize) -> u16 {
    u16::from_le(unsafe { read_volatile((VA_CFG + off) as *const u16) })
}
fn poner_cfg16(off: usize, v: u16) {
    unsafe { write_volatile((VA_CFG + off) as *mut u16, v.to_le()) }
}
fn cfg32(off: usize) -> u32 {
    u32::from_le(unsafe { read_volatile((VA_CFG + off) as *const u32) })
}
fn dma32(off: usize) -> u32 {
    u32::from_le(unsafe { read_volatile((VA_DMA + off) as *const u32) })
}
fn poner32(off: usize, v: u32) {
    unsafe { write_volatile((VA_DMA + off) as *mut u32, v.to_le()) }
}
fn poner64(off: usize, v: u64) {
    poner32(off, v as u32);
    poner32(off + 4, (v >> 32) as u32);
}
/// Una palabra de la memoria de las tramas.
fn tramas(i: usize) -> u32 {
    unsafe { read_volatile((VA_TRAMAS + 4 * i) as *const u32) }
}
fn poner_tramas(i: usize, v: u32) {
    unsafe { write_volatile((VA_TRAMAS + 4 * i) as *mut u32, v) }
}
fn sumar(i: usize) {
    poner_tramas(i, tramas(i).wrapping_add(1));
}

/// Espera girando (para las esperas de microsegundos).
fn girar_us(us: u64) {
    let fin = dashos::ahora_us() + us;
    while dashos::ahora_us() < fin {
        core::hint::spin_loop();
    }
}
fn dormir_ms(ms: u64) {
    dashos::dormir_us(ms * 1000);
}
/// Mira `cond` hasta `veces` veces, con `cada_us` entre una y otra (durmiendo si es de 1 ms o más). Verdad si se cumplió.
fn hasta(veces: u32, cada_us: u64, cond: impl Fn() -> bool) -> bool {
    for _ in 0..veces {
        if cond() {
            return true;
        }
        if cada_us >= 1000 {
            dashos::dormir_us(cada_us);
        } else {
            girar_us(cada_us);
        }
    }
    cond()
}

// --- Accesos indirectos (§3) ---

/// OCP de la MAC (§3.1): sin espera.
fn mac_ocp(a: u16) -> u16 {
    w32(OCPDR, (a as u32) << 15);
    r32(OCPDR) as u16
}
fn poner_mac_ocp(a: u16, v: u16) {
    w32(OCPDR, 0x8000_0000 | (a as u32) << 15 | v as u32);
}
/// Leer, cambiar bits y escribir un registro OCP de la MAC: quitar `quita` y poner `pone`.
fn mac_ocp_bits(a: u16, quita: u16, pone: u16) {
    poner_mac_ocp(a, (mac_ocp(a) & !quita) | pone);
}

/// OCP del PHY (§3.2): escribir y esperar a que baje el bit 31 (10 × 25 µs).
fn poner_phy(a: u16, v: u16) -> bool {
    w32(GPHY_OCP, 0x8000_0000 | (a as u32) << 15 | v as u32);
    hasta(10, 25, || r32(GPHY_OCP) & 0x8000_0000 == 0)
}
/// Leer: esperar a que suba el bit 31.
fn phy(a: u16) -> Option<u16> {
    w32(GPHY_OCP, (a as u32) << 15);
    hasta(10, 25, || r32(GPHY_OCP) & 0x8000_0000 != 0).then(|| r32(GPHY_OCP) as u16)
}
/// Leer, quitar `quita`, poner `pone` y escribir un registro del PHY. Falso si no contesta.
fn phy_bits(a: u16, quita: u16, pone: u16) -> bool {
    phy(a).is_some_and(|v| poner_phy(a, (v & !quita) | pone))
}
/// Los parámetros indirectos (§3.2).
fn param_g(n: u16) -> Option<u16> {
    poner_phy(PARAM_G_NUM, n);
    phy(PARAM_G_VAL)
}
fn poner_param_g(n: u16, v: u16) -> bool {
    poner_phy(PARAM_G_NUM, n) && poner_phy(PARAM_G_VAL, v)
}
fn poner_param_8125(n: u16, v: u16) -> bool {
    poner_phy(PARAM_8125_NUM, n) && poner_phy(PARAM_8125_VAL, v)
}
fn param_8125_bits(n: u16, quita: u16, pone: u16) -> bool {
    poner_phy(PARAM_8125_NUM, n) && phy(PARAM_8125_VAL).is_some_and(|v| poner_phy(PARAM_8125_VAL, (v & !quita) | pone))
}

/// ePHY (§3.3): escribir, esperar a que baje el bit 31 (100 × 10 µs) y 10 µs más.
fn poner_ephy(r: u32, v: u16) -> bool {
    w32(EPHYAR, 0x8000_0000 | (r & 0x7F) << 16 | v as u32);
    let bien = hasta(100, 10, || r32(EPHYAR) & 0x8000_0000 == 0);
    girar_us(10);
    bien
}
fn ephy(r: u32) -> Option<u16> {
    w32(EPHYAR, (r & 0x7F) << 16);
    hasta(100, 10, || r32(EPHYAR) & 0x8000_0000 != 0).then(|| r32(EPHYAR) as u16)
}
fn ephy_bits(r: u32, quita: u16, pone: u16) -> bool {
    ephy(r).is_some_and(|v| poner_ephy(r, (v & !quita) | pone))
}

/// El aparato, para el intérprete del firmware (§6.3).
struct Aparato;

impl Bus for Aparato {
    fn phy_leer(&mut self, dir: u16) -> Option<u16> {
        phy(dir)
    }
    fn phy_escribir(&mut self, dir: u16, v: u16) -> bool {
        poner_phy(dir, v)
    }
    fn mac_leer(&mut self, dir: u16) -> u16 {
        mac_ocp(dir)
    }
    fn mac_escribir(&mut self, dir: u16, v: u16) {
        poner_mac_ocp(dir, v)
    }
    fn esperar_ms(&mut self, ms: u32) {
        dormir_ms(ms as u64);
    }
}

/// Dónde empieza la capacidad `id` en la configuración del PCIe, si la tiene.
fn capacidad(id: u32) -> Option<usize> {
    if (cfg32(4) >> 16) & (1 << 4) == 0 {
        return None;
    }
    let mut q = (cfg32(0x34) & 0xFC) as usize;
    for _ in 0..48 {
        if q < 0x40 {
            return None;
        }
        let w = cfg32(q);
        if w & 0xFF == id {
            return Some(q);
        }
        q = ((w >> 8) & 0xFC) as usize;
    }
    None
}

// --- Arranque (§8) ---

/// Una dirección MAC que vale: ni ceros ni de multidifusión (§5).
fn mac_valida(m: &[u8; 6]) -> bool {
    *m != [0; 6] && m[0] & 1 == 0
}

fn leer6(off: usize) -> [u8; 6] {
    core::array::from_fn(|i| r8(off + i))
}

/// Escribir la MAC activa (§5): `MAC4` antes que `MAC0`, con la configuración desbloqueada.
fn poner_mac(m: &[u8; 6]) {
    desbloquear();
    w32(MAC4, m[4] as u32 | (m[5] as u32) << 8);
    confirmar();
    w32(MAC0, u32::from_le_bytes([m[0], m[1], m[2], m[3]]));
    confirmar();
    bloquear();
}

/// Esperar a que la MAC tenga las FIFO vacías (§12, pasos 3.2 y 3.3). Verdad si se vaciaron.
fn esperar_fifo() -> bool {
    let vacias = MCU_TX_EMPTY | MCU_RX_EMPTY;
    let a = hasta(42, 100, || r8(MCU) & vacias == vacias);
    let b = hasta(42, 100, || r16(INTR_MITIGATE) & 0x0103 == 0x0103);
    a && b
}

/// Reinicio de la MAC (§8, paso 6). Falso si no termina.
fn reiniciar() -> bool {
    w8(CHIP_CMD, CMD_RESET);
    hasta(100, 100, || r8(CHIP_CMD) & CMD_RESET == 0)
}

/// Salir del modo "fuera de banda" (§8, paso 5).
fn salir_de_oob() {
    activar32(MISC, MISC_RXDV_GATED);
    dormir_ms(2);
    if !esperar_fifo() {
        println!("  [rtl-red] las FIFO no se vacían al salir de OOB; se sigue");
    }
    quitar8(CHIP_CMD, CMD_RX | CMD_TX);
    dormir_ms(1);
    quitar8(MCU, MCU_NOW_IS_OOB);
    mac_ocp_bits(0xE8DE, 1 << 14, 0);
    let listo = || r8(MCU) & MCU_LINK_LIST_RDY != 0;
    let a = hasta(42, 100, listo);
    poner_mac_ocp(0xC0AA, 0x07D0);
    poner_mac_ocp(0xC0A6, 0x0150);
    poner_mac_ocp(0xC01E, 0x5555);
    let b = hasta(42, 100, listo);
    if !(a && b) {
        println!("  [rtl-red] la MAC no dice LINK_LIST_RDY al salir de OOB; se sigue");
    }
    // El PHY en "UPS" (§8, paso 5.6; una fuente).
    if mac_ocp(0xD42C) & (1 << 8) != 0 {
        if !hasta(100, 1000, || phy(PHY_ESTADO).is_some_and(|v| v & 7 == 2)) {
            println!("  [rtl-red] el PHY no llega al estado UPS; se sigue");
        }
        mac_ocp_bits(0xD42C, 1 << 8, 0);
        phy_bits(0xA466, 1 << 0, 0);
        phy_bits(0xA468, (1 << 1) | (1 << 3), 0);
    }
}

/// El PHY: encenderlo, el firmware, su configuración, el anuncio y la negociación (§6.5, §6.6). Escribe en el
/// registro lo que se lee para comparar con el hardware (§16).
fn preparar_phy(firmware: Option<&fw::Firmware>) {
    activar8(PMCH, 1 << 6);
    poner_phy(BMCR, 0x1000);
    if !hasta(100, 1000, || phy(PHY_ESTADO).is_some_and(|v| v & 7 == 3)) {
        println!("  [rtl-red] el PHY no dice \"LAN on\" ({:#x}); se sigue", phy(PHY_ESTADO).unwrap_or(0xFFFF));
    }
    dormir_ms(20);
    let id = (phy(PHYID1).unwrap_or(0xFFFF) as u32) << 16 | phy(PHYID2).unwrap_or(0xFFFF) as u32;
    let de_25 = phy(0xA616).is_some_and(|v| v & (1 << 13) != 0);
    println!("  [rtl-red] PHY {:08x}{}", id, if de_25 { ", de 2,5 Gb/s" } else { ", sin 2,5 Gb/s" });
    // El firmware (§6.3), con la versión del parche antes y después (§16).
    let antes = param_g(0x801E);
    match firmware {
        Some(f) => match f.ejecutar(&mut Aparato) {
            Ok(r) => {
                let reinicio = hasta(12, 50_000, || phy(BMCR).is_some_and(|v| v & (1 << 15) == 0));
                println!(
                    "  [rtl-red] firmware aplicado: {} acciones, {} escrituras; parche del PHY {:#06x} -> {:#06x}{}",
                    r.pasos,
                    r.escrituras,
                    antes.unwrap_or(0xFFFF),
                    param_g(0x801E).unwrap_or(0xFFFF),
                    if reinicio { "" } else { "; el PHY no termina su reinicio" }
                );
            }
            Err(e) => println!("  [rtl-red] el firmware falla al aplicarlo ({:?}); se sigue sin terminarlo", e),
        },
        None => println!("  [rtl-red] sin firmware del PHY: se sigue sin él (parche {:#06x})", antes.unwrap_or(0xFFFF)),
    }
    // La configuración de la 8125B (§6.5, pasos 2 a 12).
    let mut bien = phy_bits(0xA442, 0, 1 << 11);
    bien &= phy_bits(0xAC46, 0xF << 4, 0x9 << 4);
    bien &= phy_bits(0xAD30, 0x3, 0x1);
    bien &= poner_param_8125(0x80F5, 0x760E);
    bien &= poner_param_8125(0x8107, 0x360E);
    bien &= param_8125_bits(0x8551, 0xFF00, 0x0800);
    bien &= phy_bits(0xBF00, 0x7 << 13, 0b101 << 13);
    bien &= phy_bits(0xBF46, 0xF << 8, 3 << 8);
    for n in (0x8044..=0x807A).step_by(6) {
        bien &= poner_param_g(n, 0x2417);
    }
    bien &= phy_bits(0xA4CA, 0, 1 << 6);
    bien &= phy_bits(0xBF84, 0x7 << 13, 0b101 << 13);
    bien &= phy_bits(0xA5B4, 1 << 15, 0);
    bien &= phy_bits(0xA430, 1 << 2, 0);
    // EEE apagado, como recomienda la especificación (§6.5, "EEE").
    mac_ocp_bits(0xE040, 0x3, 0);
    bien &= phy_bits(0xA432, 0, 1 << 4);
    bien &= phy_bits(0xA5D0, (1 << 1) | (1 << 2), 0);
    bien &= phy_bits(0xA6D4, 1 << 0, 0);
    bien &= phy_bits(0xA6D8, 1 << 4, 0);
    bien &= phy_bits(0xA428, 1 << 7, 0);
    bien &= phy_bits(0xA4A2, 1 << 9, 0);
    mac_ocp_bits(0xE052, 1 << 0, 0);
    bien &= phy_bits(0xA442, (1 << 12) | (1 << 13), 0);
    bien &= phy_bits(0xA430, 1 << 15, 0);
    // El anuncio (§6.6, paso 4): 10, 100, 1000 y 2500, sin pausa.
    bien &= poner_phy(ANAR, 0x01E1);
    bien &= phy_bits(GBCR, 0, 1 << 9);
    bien &= phy_bits(0xA5D4, (1 << 8) | (1 << 12), 1 << 7);
    if !bien {
        println!("  [rtl-red] el PHY no contesta a toda la configuración; se sigue");
    }
    // Reiniciar y negociar (§6.6, paso 5).
    poner_phy(BMCR, 0x9200);
    if !hasta(50, 50_000, || phy(BMCR).is_some_and(|v| v & (1 << 15) == 0)) {
        println!("  [rtl-red] el PHY no termina su reinicio tras la negociación");
    }
}

/// Poner en marcha (§8 C, pasos 11 a 26) con los anillos ya preparados en la DMA.
fn poner_en_marcha(bus: u64) {
    desbloquear();
    quitar8(CONFIG2, 1 << 7);
    quitar8(CONFIG5, 1 << 0);
    w16(CPLUS_CMD, r16(CPLUS_CMD) & !((1 << 5) | (1 << 6)));
    w16(0x6048, 1500 + 14 + 32);
    w8(INT_CFG0, 0);
    for i in 0..32 {
        w32(0xA00 + 4 * i, 0);
    }
    w16(INT_CFG1, 0);
    mac_ocp_bits(0xEA84, 0, 0x3);
    // El paso 17 (latencia de entrada en L0s y L1) se omite: ASPM está apagado (§8, paso 17).
    // El ePHY de la 8125B (paso 18).
    let mut bien = poner_ephy(0x0B, 0xA908);
    bien &= poner_ephy(0x1E, 0x20EB);
    bien &= poner_ephy(0x4B, 0xA908);
    bien &= poner_ephy(0x5E, 0x20EB);
    bien &= ephy_bits(0x22, 0x3 << 4, 2 << 4);
    bien &= ephy_bits(0x62, 0x3 << 4, 2 << 4);
    if !bien {
        println!("  [rtl-red] el ePHY no contesta; se sigue");
    }
    // La parte común (paso 19).
    quitar8(CONFIG3, 1 << 1);
    w16(0x382, 0x221B);
    w32(0x4500, 0);
    w16(0x4800, 0);
    mac_ocp_bits(0xD40A, 1 << 4, 0);
    quitar8(CONFIG1, 1 << 4);
    poner_mac_ocp(0xC140, 0xFFFF);
    poner_mac_ocp(0xC142, 0xFFFF);
    mac_ocp_bits(0xD3E2, 0x0FFF, 0x03A9);
    mac_ocp_bits(0xD3E4, 0x00FF, 0);
    mac_ocp_bits(0xE860, 0, 1 << 7);
    mac_ocp_bits(0xEB58, 1 << 0, 0);
    mac_ocp_bits(0xE614, 0x7 << 8, 2 << 8);
    mac_ocp_bits(0xE63E, (1 << 4) | (1 << 5) | (1 << 10) | (1 << 11), 0);
    mac_ocp_bits(0xC0B4, 0, (1 << 2) | (1 << 3));
    mac_ocp_bits(0xEB6A, 0x00FF, 0x0033);
    mac_ocp_bits(0xEB50, 0x1F << 5, 2 << 5);
    mac_ocp_bits(0xE056, 0xF << 4, 0);
    mac_ocp_bits(0xE040, 1 << 12, 0);
    mac_ocp_bits(0xEA1C, 0x3 | (1 << 2), 0x1);
    mac_ocp_bits(0xE0C0, 0x4F0F, 0x4403);
    mac_ocp_bits(0xE052, 1 << 7, (1 << 3) | (1 << 5) | (1 << 6));
    mac_ocp_bits(0xD430, 0x0FFF, 0x047F);
    mac_ocp_bits(0xEB54, 0, 1 << 0);
    girar_us(1);
    mac_ocp_bits(0xEB54, 1 << 0, 0);
    w16(0x1880, r16(0x1880) & !((1 << 4) | (1 << 5)));
    poner_mac_ocp(0xE098, 0xC302);
    if !hasta(10, 1000, || mac_ocp(0xE00E) & (1 << 13) == 0) {
        println!("  [rtl-red] la MAC no baja el bit 13 de 0xE00E; se sigue");
    }
    quitar32(MISC, MISC_RXDV_GATED);
    // Salidas de L1 (paso 20); ASPM y CLKREQ se quedan apagados (paso 21).
    mac_ocp_bits(0xC0AC, 0, 0x1F80);
    // Tamaño máximo y anillos (paso 22): la parte alta antes que la baja (§7.4).
    w16(RX_MAX_SIZE, BUFER as u16);
    let (tx, rx) = (bus + M_TX as u64, bus + M_RX as u64);
    w32(TX_DESC_ALTO, (tx >> 32) as u32);
    w32(TX_DESC_BAJO, tx as u32);
    w32(RX_DESC_ALTO, (rx >> 32) as u32);
    w32(RX_DESC_BAJO, rx as u32);
    bloquear();
    confirmar();
    // Encender (pasos 23 a 25): toda la multidifusión (§10), `MAR4` antes que `MAR0`.
    w8(CHIP_CMD, CMD_RX | CMD_TX);
    w32(RX_CONFIG, RXCFG_BASE | RXCFG_ACEPTAR);
    w32(TX_CONFIG, TXCFG);
    w32(MAR4, u32::MAX);
    w32(MAR0, u32::MAX);
    confirmar();
}

/// Los anillos, como al empezar (§7.3, §8 paso 8): todos los de recibir del aparato; los de enviar, del controlador.
fn preparar_anillos(bus: u64) {
    for i in 0..N_TX {
        poner64(M_TX + 16 * i, 0);
        poner64(M_TX + 16 * i + 8, 0);
    }
    for i in 0..N_RX {
        let d = M_RX + 16 * i;
        poner32(d + 4, 0);
        poner64(d + 8, bus + (M_RX_BUF + i * BUFER) as u64);
    }
    dashos::barrera::total();
    for i in 0..N_RX {
        devolver_rx(i);
    }
    dashos::barrera::total();
}

/// Dar el descriptor de recibir `i` al aparato (§7.3): la dirección se queda; `opts1` con `DescOwn`, el tamaño del
/// búfer y `RingEnd` en el último.
fn devolver_rx(i: usize) {
    let fin = if i == N_RX - 1 { RING_END } else { 0 };
    poner32(M_RX + 16 * i, OWN | fin | BUFER as u32);
}

/// La parada (§12). Con `final_`, también el PHY apagado, la MAC de fábrica y sin maestro de bus: después solo queda
/// soltar la DMA. Sin él (para volver a arrancar en marcha), hasta el reinicio.
fn parar(fabrica: &[u8; 6], final_: bool) {
    // Un aparato que ya no responde lee todo a unos: no hay nada que hacer sin un reinicio del PCI (§13).
    if r32(TX_CONFIG) != u32::MAX {
        w32(INTR_MASK, 0);
        w32(INTR_STATUS, u32::MAX);
        confirmar();
        quitar32(RX_CONFIG, 0x3F);
        activar32(MISC, MISC_RXDV_GATED);
        dormir_ms(2);
        activar8(CHIP_CMD, CMD_STOP_REQ);
        if !esperar_fifo() {
            println!("  [rtl-red] las FIFO no se vacían al parar; se sigue");
        }
        dormir_ms(2);
        if !reiniciar() {
            println!("  [rtl-red] la MAC no termina el reinicio al parar");
        }
        if !final_ {
            return;
        }
        mac_ocp_bits(0xC0AC, 0x1F80, 0);
        poner_mac(fabrica);
        // El PHY, apagado: no hay despertar por LAN (§12, paso 8).
        phy_bits(BMCR, 0, 1 << 11);
    } else if !final_ {
        return;
    }
    poner_cfg16(4, cfg16(4) & !(1 << 2));
}

fn principal() -> u64 {
    // Con llave 6, la 5 es la interrupción y la 6 el canal "red"; si no, el canal es la 5.
    let existe = |k: u32| {
        let copia = dashos::llave_duplicar(k, dashos::LEER, 0);
        if copia > 0 {
            dashos::cerrar(copia as u32);
        }
        copia > 0
    };
    let (irq, servicio) = if existe(6) { (Some(5u32), 6u32) } else { (None, 5u32) };
    if dashos::registros_proyectar(2, VA) != 0 || dashos::registros_proyectar(3, VA_CFG) != 0 {
        println!("  [rtl-red] sin registros");
        return 1;
    }
    let id = cfg32(0);
    if id != 0x8125_10EC {
        println!("  [rtl-red] aparato {:04x}:{:04x} sin soporte", id & 0xFFFF, id >> 16);
        return 2;
    }
    // En D0 (§2.1): la capacidad de energía del PCI, con 10 ms si estaba dormido.
    if let Some(c) = capacidad(0x01) {
        let pm = cfg16(c + 4);
        if pm & 3 != 0 {
            poner_cfg16(c + 4, pm & !3);
            dormir_ms(10);
        }
    }
    // La versión (§2.2).
    let txc = r32(TX_CONFIG);
    if txc == u32::MAX {
        println!("  [rtl-red] el aparato no responde (TxConfig a unos)");
        return 2;
    }
    let xid = (txc >> 20) & 0x7CF;
    if xid != 0x641 {
        println!("  [rtl-red] RTL8125 versión XID {:#05x} sin soporte", xid);
        return 2;
    }
    if mac_ocp(0xD006) & 0xFF == 0x07 {
        println!("  [rtl-red] RTL8125B en modo SFP: sin soporte");
        return 2;
    }
    // Sin ASPM (L0s, L1) ni el reloj por CLKREQ en el control del enlace del aparato (§8, paso 2).
    if let Some(c) = capacidad(0x10) {
        poner_cfg16(c + 0x10, cfg16(c + 0x10) & !((1 << 0) | (1 << 1) | (1 << 8)));
    }
    w32(RX_CONFIG, RXCFG_BASE);
    w32(INTR_MASK, 0);
    w32(INTR_STATUS, u32::MAX);
    confirmar();
    salir_de_oob();
    if !reiniciar() {
        println!("  [rtl-red] la MAC no termina el reinicio");
        return 2;
    }
    // La MAC (§5): la de fábrica; si no vale, la activa; si tampoco, una local al azar.
    let fabrica = leer6(MAC0_BKP);
    let mut mac = if mac_valida(&fabrica) { fabrica } else { leer6(MAC0) };
    if !mac_valida(&mac) {
        let mut a = [0u8; 6];
        dashos::azar(&mut a);
        mac = [(a[0] & 0xFC) | 0x02, a[1], a[2], a[3], a[4], a[5]];
        println!("  [rtl-red] sin dirección MAC de fábrica; se usa una local");
    }
    let fabrica = if mac_valida(&fabrica) { fabrica } else { mac };
    poner_mac(&mac);

    // La memoria DMA (§8, paso 8).
    let (mem_dma, bus) = dashos::memoria_dma(1, TAM_DMA);
    if mem_dma <= 0 || dashos::memoria_proyectar(mem_dma as u32, VA_DMA, true) != 0 {
        println!("  [rtl-red] sin memoria DMA");
        return 3;
    }
    for i in (0..TAM_DMA).step_by(4) {
        poner32(i, 0);
    }
    preparar_anillos(bus);
    // El firmware (llave 4): validado entero antes de usarlo (§6.3). Si falta o no vale, se sigue sin él (§6.2).
    let tam_fw = dashos::memoria_tamano(4);
    let datos: &[u8] = if tam_fw >= 4 && dashos::memoria_proyectar(4, VA_FW, false) == 0 {
        let n = unsafe { read_volatile(VA_FW as *const u32) } as usize;
        if n <= tam_fw as usize - 4 { unsafe { core::slice::from_raw_parts((VA_FW + 4) as *const u8, n) } } else { &[] }
    } else {
        &[]
    };
    let firmware = if datos.is_empty() {
        println!("  [rtl-red] el sistema no tiene el firmware del PHY (rtl8125b-2.fw)");
        None
    } else {
        match fw::validar(datos) {
            Ok(f) => {
                let v = f.version.and_then(|v| core::str::from_utf8(v).ok()).unwrap_or("sin versión");
                println!("  [rtl-red] firmware del PHY: {} ({} bytes, {} acciones)", v, datos.len(), f.acciones());
                Some(f)
            }
            Err(e) => {
                println!("  [rtl-red] el firmware del PHY no vale ({:?}); se sigue sin él", e);
                None
            }
        }
    };
    preparar_phy(firmware.as_ref());
    poner_en_marcha(bus);

    // La memoria de las tramas, para el servicio.
    let mem = dashos::memoria_crear(TAM_TRAMAS);
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_TRAMAS, true) != 0 {
        println!("  [rtl-red] sin memoria para las tramas");
        parar(&fabrica, true);
        return 4;
    }
    poner_tramas(p::T_RANURAS_RX, RANURAS_RX as u32);
    poner_tramas(p::T_RANURAS_TX, RANURAS_TX as u32);
    // El canal propio: la interrupción (marca 1) y los avisos del servicio (marca 2).
    let (lee, escribe) = dashos::canal_crear();
    let para_servicio = dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, M_SERVICIO);
    let copia = dashos::llave_duplicar(mem, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let mut hola = [0u8; 12];
    hola[..4].copy_from_slice(&p::TARJETA.to_le_bytes());
    hola[4..10].copy_from_slice(&mac);
    if para_servicio <= 0 || copia <= 0 || dashos::canal_escribir(servicio, &hola, &[copia as u32, para_servicio as u32]) != 0 {
        println!("  [rtl-red] el servicio no recibe la tarjeta");
        parar(&fabrica, true);
        return 5;
    }
    let irq = irq.filter(|&i| {
        let marcada = dashos::llave_duplicar(escribe, dashos::ESCRIBIR, M_IRQ);
        marcada > 0 && dashos::interrupcion_asociar(i, marcada as u32) == 0
    });
    w32(INTR_STATUS, u32::MAX);
    w32(INTR_MASK, MASCARA);
    println!(
        "  [rtl-red] RTL8125B (10ec:8125, XID {:#05x}); MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}; colas de {} y {}; avisos {}",
        xid,
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], N_RX, N_TX,
        if irq.is_some() { "por su interrupción (MSI-X)" } else { "con un temporizador" }
    );

    // El estado de los anillos: el siguiente de recibir que mirar, el siguiente de enviar libre y el más antiguo en
    // vuelo. Y si se está tirando una trama partida (sin `LastFrag`).
    let mut rx_sig = 0usize;
    let mut tx_cola = 0usize;
    let mut tx_limpio = 0usize;
    let mut partida = false;
    let mut con_enlace = false;
    let mut proxima_mirada = 0u64;
    let (mut rotas, mut tx_errores) = (0u64, 0u64);
    let mut buf = [0u8; 16];
    let mut ll = [0u32; 8];
    loop {
        let plazo = if irq.is_some() { 100_000 } else { 2_000 };
        let (n, k, marca) = dashos::canal_leer_plazo(lee, &mut buf, &mut ll, plazo);
        let k = if n >= 0 { k.min(ll.len()) } else { 0 };
        ll[..k].iter().for_each(|&l| {
            dashos::cerrar(l);
        });
        // Las causas (§11): con la máscara a 0 mientras se atienden; se borran escribiendo lo leído.
        let causas = r32(INTR_STATUS);
        if causas & 0xFFFF == 0xFFFF {
            println!("  [rtl-red] el aparato ha desaparecido (IntrStatus a unos); se para");
            parar(&fabrica, true);
            return 6;
        }
        if causas != 0 {
            w32(INTR_MASK, 0);
            w32(INTR_STATUS, causas);
        }
        let mut movido = false;
        if causas & I_SYSERR != 0 {
            // Error del bus: parada y arranque en marcha con la misma DMA (§11); lo que estaba en vuelo se pierde.
            let perdidas = (tx_cola + N_TX - tx_limpio) % N_TX;
            println!("  [rtl-red] error del bus (SYSErr); se reinicia la tarjeta ({} tramas de enviar perdidas)", perdidas);
            parar(&fabrica, false);
            poner_mac(&mac);
            preparar_anillos(bus);
            poner_en_marcha(bus);
            w32(INTR_STATUS, u32::MAX);
            (rx_sig, tx_cola, tx_limpio, partida, con_enlace) = (0, 0, 0, false, false);
            movido = true;
        }
        // El enlace (§9): al cambiar o cada segundo.
        let ahora = dashos::ahora_us();
        if causas & I_LINKCHG != 0 || ahora >= proxima_mirada {
            proxima_mirada = ahora + 1_000_000;
            let st = r16(PHY_STATUS);
            let arriba = st & (1 << 1) != 0;
            if arriba != con_enlace {
                con_enlace = arriba;
                if arriba {
                    let mbs = if st & (1 << 10) != 0 {
                        2500
                    } else if st & (1 << 4) != 0 {
                        1000
                    } else if st & (1 << 3) != 0 {
                        100
                    } else {
                        10
                    };
                    let duplex = st & 1 != 0 || mbs >= 1000;
                    println!("  [rtl-red] enlace arriba: {} Mb/s, {}", mbs, if duplex { "dúplex" } else { "semidúplex" });
                } else {
                    println!("  [rtl-red] enlace abajo");
                }
            }
        }
        if n >= 0 && marca == M_IRQ {
            if let Some(i) = irq {
                dashos::interrupcion_terminar(i);
            }
        }
        // Lo recibido (§7.3): al anillo del servicio, si cabe y tiene buena medida.
        for _ in 0..N_RX {
            let d = M_RX + 16 * rx_sig;
            let opts1 = dma32(d);
            if opts1 & OWN != 0 {
                break;
            }
            dashos::barrera::lecturas();
            sumar(p::T_RECIBIDAS);
            let entera = opts1 & FIRST != 0 && opts1 & LAST != 0;
            let largo = ((opts1 & 0x3FFF) as usize).saturating_sub(4);
            if partida || !entera {
                // Una trama que no cabe en un búfer se tira entera, hasta el descriptor con `LastFrag`.
                partida = opts1 & LAST == 0;
                sumar(p::T_LARGAS);
            } else if opts1 & RX_RES != 0 {
                if opts1 & RX_RWT != 0 {
                    sumar(p::T_LARGAS);
                } else if opts1 & RX_RUNT != 0 {
                    sumar(p::T_CORTAS);
                } else {
                    // CRC u otro error: no hay contador `T_*` para esto.
                    rotas += 1;
                    if rotas == 1 {
                        println!("  [rtl-red] llega una trama con error ({:#010x}); se tiran", opts1);
                    }
                }
            } else if largo < p::MIN_TRAMA {
                sumar(p::T_CORTAS);
            } else if largo > p::MAX_TRAMA || largo > BUFER {
                sumar(p::T_LARGAS);
            } else {
                let (escritas, leidas) = (tramas(p::T_RX_ESCRITAS), tramas(p::T_RX_LEIDAS));
                if escritas.wrapping_sub(leidas) as usize >= RANURAS_RX {
                    sumar(p::T_SIN_SITIO);
                } else {
                    let r = VA_TRAMAS + 64 + (escritas as usize % RANURAS_RX) * p::RANURA;
                    let desde = VA_DMA + M_RX_BUF + rx_sig * BUFER;
                    // De la DMA a la memoria compartida: bytes, sin interpretarlos.
                    unsafe { core::ptr::copy_nonoverlapping(desde as *const u8, (r + p::RANURA_DATOS) as *mut u8, largo) };
                    unsafe { write_volatile(r as *mut u32, largo as u32) };
                    dashos::barrera::total();
                    poner_tramas(p::T_RX_ESCRITAS, escritas.wrapping_add(1));
                    movido = true;
                }
            }
            // Devolverlo al aparato en cuanto se copia.
            poner32(d + 4, 0);
            dashos::barrera::total();
            devolver_rx(rx_sig);
            rx_sig = (rx_sig + 1) % N_RX;
        }
        // Lo enviado (§7.2): libre desde el más antiguo mientras el aparato lo haya devuelto.
        while tx_limpio != tx_cola {
            let opts1 = dma32(M_TX + 16 * tx_limpio);
            if opts1 & OWN != 0 {
                break;
            }
            if opts1 & TX_ERROR != 0 {
                tx_errores += 1;
                if tx_errores == 1 {
                    println!("  [rtl-red] error al enviar una trama ({:#010x})", opts1);
                }
            }
            tx_limpio = (tx_limpio + 1) % N_TX;
        }
        // Lo que el servicio quiere enviar, mientras haya descriptores libres (siempre uno sin usar).
        let mut puestos = false;
        loop {
            let (escritas, leidas) = (tramas(p::T_TX_ESCRITAS), tramas(p::T_TX_LEIDAS));
            if escritas == leidas {
                break;
            }
            // El servicio no puede tener más ranuras llenas que las que hay: si lo dice, se tira todo.
            if escritas.wrapping_sub(leidas) as usize > RANURAS_TX {
                sumar(p::T_TX_MALAS);
                poner_tramas(p::T_TX_LEIDAS, escritas);
                movido = true;
                break;
            }
            if (tx_cola + 1) % N_TX == tx_limpio {
                break;
            }
            dashos::barrera::total();
            let r = VA_TRAMAS + 64 + (RANURAS_RX + leidas as usize % RANURAS_TX) * p::RANURA;
            let largo = unsafe { read_volatile(r as *const u32) } as usize;
            if (p::MIN_TRAMA..=p::MAX_TRAMA).contains(&largo) {
                let dest = M_TX_BUF + tx_cola * BUFER;
                unsafe { core::ptr::copy_nonoverlapping((r + p::RANURA_DATOS) as *const u8, (VA_DMA + dest) as *mut u8, largo) };
                // Relleno con ceros hasta 60 bytes (§7.2); el CRC lo pone el aparato.
                let largo_dma = largo.max(60);
                if largo < largo_dma {
                    unsafe { core::ptr::write_bytes((VA_DMA + dest + largo) as *mut u8, 0, largo_dma - largo) };
                }
                let d = M_TX + 16 * tx_cola;
                poner32(d + 4, 0);
                poner64(d + 8, bus + dest as u64);
                dashos::barrera::total();
                let fin = if tx_cola == N_TX - 1 { RING_END } else { 0 };
                poner32(d, OWN | fin | FIRST | LAST | largo_dma as u32);
                tx_cola = (tx_cola + 1) % N_TX;
                sumar(p::T_ENVIADAS);
                puestos = true;
            } else {
                sumar(p::T_TX_MALAS);
            }
            poner_tramas(p::T_TX_LEIDAS, leidas.wrapping_add(1));
            movido = true;
        }
        // El timbre (§7.2): también si quedan pendientes, porque el aparato puede perder un toque.
        if puestos || tx_limpio != tx_cola {
            dashos::barrera::total();
            w16(TX_POLL, 1);
        }
        if causas != 0 {
            w32(INTR_MASK, MASCARA);
        }
        if movido {
            // Si la cola del servicio está llena, se pierde: el servicio mira las ranuras en cada vuelta. Si el
            // servicio ya no está, la parada ordenada (§12) y fuera.
            if dashos::canal_escribir(servicio, &p::TRAMAS.to_le_bytes(), &[]) == dashos::E_CERRADO {
                println!("  [rtl-red] el servicio de red se fue; se para la tarjeta");
                parar(&fabrica, true);
                return 0;
            }
        }
    }
}
