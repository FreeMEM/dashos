//! El controlador de red de Intel (X5c, red con cable): la MAC del PCH con el PHY I219 (I219-V de pegaso, familia
//! "Cannon Point") y la tarjeta 82574L, que es la que emula QEMU con `-device e1000e`. Un solo programa: lo común (los
//! registros de la MAC, los descriptores clásicos, el acceso al PHY por MDIC) y las diferencias por el ID del PCI.
//! Escrito con la especificación de sala limpia `docs/spec/hw/i219.md` (las secciones se citan como "§"). Nada de
//! Linux, BSD ni QEMU.
//!
//! Llaves, como `virtio-red`: la 1, su permiso DMA; la 2, sus registros (la BAR 0); la 3, su espacio de configuración
//! del PCIe; la 4, su interrupción (MSI-X entrada 0 en el 82574, MSI en el I219), si la hay; la siguiente, el canal
//! "red" (el servicio). Sin interrupción, mira los anillos cada 2 ms.
//!
//! Hacia el servicio, lo mismo que `virtio-red` (§1): `TARJETA` con la MAC, la memoria de las tramas y un canal suyo;
//! `TRAMAS` cuando mueve algo. Una cola de recibir de 128 búferes de 2 KB y una de enviar de 64, con descriptores
//! clásicos (§7). Sin sumas de comprobación por el aparato, sin TSO, sin VLAN, sin control de flujo, sin EEE, toda la
//! multidifusión (`MPE`; filtra el servicio). Copia cada trama entre la DMA y las ranuras: el servicio nunca ve los
//! descriptores. Tira y cuenta lo corto, lo largo, lo que no cabe y lo que llega con error o partido; y suma a esas
//! cuentas lo que tira la tarjeta misma (sus contadores, §11.1). Si el servicio se va, la parada ordenada (§12); si
//! el PHY del 82574 se cuelga, parada y arranque en marcha con la misma DMA (§14.3).
//!
//! Lo propio del I219 (el ME, el semáforo, el reposo ULP, el modo SMBus, el ciclo LANPHYPC, la NVM en la flash, la
//! configuración del PHY desde ella, el vaciado de los anillos y los ajustes al subir el enlace) está escrito según la
//! especificación, con sus tiempos máximos y sin esperas infinitas, pero **sin probar en hardware**: QEMU no lo emula.

#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use dashos::println;
use dashos::protocolo::red as p;

dashos::programa!(principal);

const VA: usize = 0x10_0000_0000;
const VA_CFG: usize = 0x10_1000_0000;
const VA_DMA: usize = 0x30_0000_0000;
const VA_TRAMAS: usize = 0x40_0000_0000;

// La memoria DMA: los dos anillos (alineados a 128 bytes y de un múltiplo de 128, §7) y los búferes.
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

// Registros comunes (§3.1).
const CTRL: usize = 0x0000;
const STATUS: usize = 0x0008;
const EECD: usize = 0x0010;
const EERD: usize = 0x0014;
const CTRL_EXT: usize = 0x0018;
const MDIC: usize = 0x0020;
const ICR: usize = 0x00C0;
const ITR: usize = 0x00C4;
const IMS: usize = 0x00D0;
const IMC: usize = 0x00D8;
const RCTL: usize = 0x0100;
const TCTL: usize = 0x0400;
const TIPG: usize = 0x0410;
const LEDCTL: usize = 0x0E00;
const EXTCNF_CTRL: usize = 0x0F00;
const PBA: usize = 0x1000;
const RDBAL: usize = 0x2800;
const RDBAH: usize = 0x2804;
const RDLEN: usize = 0x2808;
const RDH: usize = 0x2810;
const RDT: usize = 0x2818;
const RDTR: usize = 0x2820;
const RXDCTL: usize = 0x2828;
const RADV: usize = 0x282C;
const TDBAL: usize = 0x3800;
const TDBAH: usize = 0x3804;
const TDLEN: usize = 0x3808;
const TDH: usize = 0x3810;
const TDT: usize = 0x3818;
const TIDV: usize = 0x3820;
const TXDCTL: usize = 0x3828;
const TADV: usize = 0x382C;
const TARC0: usize = 0x3840;
const TXDCTL1: usize = 0x3928;
const TARC1: usize = 0x3940;
const RXCSUM: usize = 0x5000;
const RFCTL: usize = 0x5008;
const MTA: usize = 0x5200;
const RAL0: usize = 0x5400;
const RAH0: usize = 0x5404;
// Contadores de lo que tira la tarjeta (§11.1): se borran al leerlos.
const MPC: usize = 0x4010;
const RUC: usize = 0x40A4;
const ROC: usize = 0x40AC;
const RJC: usize = 0x40B0;

// Solo del I219 (§3.2).
const STRAP: usize = 0x000C;
const FEXTNVM4: usize = 0x0024;
const FEXTNVM: usize = 0x0028;
const FEXTNVM3: usize = 0x003C;
const FEXTNVM7: usize = 0x00E4;
const EXTCNF_SIZE: usize = 0x0F08;
const PHY_CTRL: usize = 0x0F10;
const PBECCSTS: usize = 0x100C;
const KABGTXD: usize = 0x3004;
const H2ME: usize = 0x5B50;
const FWSM: usize = 0x5B54;
const FEXTNVM11: usize = 0x5BBC;
const HSFSTS: usize = 0xE004;
const FADDR: usize = 0xE008;
const FDATA0: usize = 0xE010;

// Solo del 82574 (§14.3, §14.4).
const IVAR: usize = 0x00E4;
const EIAC: usize = 0x00DC;
const EITR0: usize = 0x00E8;
const GCR: usize = 0x5B00;
const GCR2: usize = 0x5B64;

// Bits (§3.3).
const CTRL_GIO_MASTER_DISABLE: u32 = 1 << 2;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_FRCSPD: u32 = 1 << 11;
const CTRL_FRCDPX: u32 = 1 << 12;
const CTRL_LANPHYPC_OVERRIDE: u32 = 1 << 16;
const CTRL_LANPHYPC_VALUE: u32 = 1 << 17;
const CTRL_MEHE: u32 = 1 << 19;
const CTRL_RST: u32 = 1 << 26;
const CTRL_RFCE: u32 = 1 << 27;
const CTRL_TFCE: u32 = 1 << 28;
const CTRL_PHY_RST: u32 = 1 << 31;
const STATUS_FD: u32 = 1 << 0;
const STATUS_LU: u32 = 1 << 1;
const STATUS_LAN_INIT_DONE: u32 = 1 << 9;
const STATUS_PHYRA: u32 = 1 << 10;
const STATUS_GIO_MASTER_ENABLE: u32 = 1 << 19;
const EXT_LPCD: u32 = 1 << 2;
const EXT_FORCE_SMBUS: u32 = 1 << 11;
const EXT_RO_DIS: u32 = 1 << 17;
const EXT_PHYPDEN: u32 = 1 << 20;
const EXT_BIT22: u32 = 1 << 22;
const EXT_EIAME: u32 = 1 << 24;
const EXT_IAME: u32 = 1 << 27;
const EXT_DRV_LOAD: u32 = 1 << 28;
const EXT_PBA_CLR: u32 = 1 << 31;
const SWFLAG: u32 = 1 << 5;
const GATE_PHY_CFG: u32 = 1 << 7;
const RCTL_EN: u32 = 1 << 1;
const RCTL_MPE: u32 = 1 << 4;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT: u32 = 0xFF << 4;
const TCTL_RTLC: u32 = 1 << 24;
const TCTL_MULR: u32 = 1 << 28;
const FWSM_RSPCIPHY: u32 = 1 << 6;
const FWSM_ULP_CFG_DONE: u32 = 1 << 10;
const FWSM_FW_VALID: u32 = 1 << 15;
const I_LSC: u32 = 1 << 2;

// El PHY: registros estándar (página 0) y los del I219 (§4.3).
const BMCR: (u16, u32) = (0, 0);
const BMSR: (u16, u32) = (0, 1);
const PHYID1: (u16, u32) = (0, 2);
const PHYID2: (u16, u32) = (0, 3);
const ANAR: (u16, u32) = (0, 4);
const GBCR: (u16, u32) = (0, 9);
const I82577_PHY_CTRL_2: (u16, u32) = (0, 18);
const I82577_CFG: (u16, u32) = (0, 22);
const EMI_DIR: (u16, u32) = (0, 16);
/// Del 82574: el control específico de su PHY (§14.3).
const PHY_ESPECIFICO: (u16, u32) = (0, 16);
const EMI_DATO: (u16, u32) = (0, 17);
const HV_OEM_BITS: (u16, u32) = (768, 25);
const HV_SMB_ADDR: (u16, u32) = (768, 26);
const HV_LED_CONFIG: (u16, u32) = (768, 30);
const BM_PORT_GEN_CFG: (u16, u32) = (769, 17);
const CV_SMB_CTRL: (u16, u32) = (769, 23);
const HV_PM_CTRL: (u16, u32) = (770, 17);
const I82579_LPI_CTRL: (u16, u32) = (772, 20);
const I217_PLL_CLOCK_GATE: (u16, u32) = (772, 28);
const P776_20: (u16, u32) = (776, 20);
const I218_ULP_CONFIG1: (u16, u32) = (779, 16);
const I217_RX_CONFIG: u16 = 0xB20C;

// Descriptores (§7).
const TX_EOP: u8 = 1 << 0;
const TX_IFCS: u8 = 1 << 1;
const TX_RS: u8 = 1 << 3;
const DD: u8 = 1 << 0;
const RX_EOP: u8 = 1 << 1;

/// Marcas de lo que llega por el canal propio.
const M_IRQ: u64 = 1;
const M_SERVICIO: u64 = 2;

fn reg32(off: usize) -> u32 {
    u32::from_le(unsafe { read_volatile((VA + off) as *const u32) })
}
fn poner_reg32(off: usize, v: u32) {
    unsafe { write_volatile((VA + off) as *mut u32, v.to_le()) }
}
fn activar(off: usize, bits: u32) {
    poner_reg32(off, reg32(off) | bits);
}
fn quitar(off: usize, bits: u32) {
    poner_reg32(off, reg32(off) & !bits);
}
/// Vaciar las escrituras de antes (§0): leer `STATUS`.
fn vaciar() {
    reg32(STATUS);
}
fn cfg32(off: usize) -> u32 {
    u32::from_le(unsafe { read_volatile((VA_CFG + off) as *const u32) })
}
fn cfg16(off: usize) -> u16 {
    u16::from_le(unsafe { read_volatile((VA_CFG + off) as *const u16) })
}
fn poner_cfg16(off: usize, v: u16) {
    unsafe { write_volatile((VA_CFG + off) as *mut u16, v.to_le()) }
}
fn dma8(off: usize) -> u8 {
    unsafe { read_volatile((VA_DMA + off) as *const u8) }
}
fn dma16(off: usize) -> u16 {
    u16::from_le(unsafe { read_volatile((VA_DMA + off) as *const u16) })
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

#[derive(Clone, Copy, PartialEq)]
enum Modelo {
    /// La MAC del PCH con el PHY I219, familia `pch_cnp` (§2.1).
    I219,
    /// El 82574L (§14).
    M82574,
}

/// Lo que se sabe del aparato.
struct Tarjeta {
    modelo: Modelo,
    /// I219: hay firmware del ME en marcha (`FWSM.FW_VALID`, §2.2).
    con_me: bool,
    /// I219: el ME bloquea el reinicio del PHY (`FWSM.RSPCIPHY` a 0, §6.3).
    bloqueado: bool,
    /// La dirección del bus de los anillos (para el vaciado del I219, §13.1).
    bus: u64,
}

impl Tarjeta {
    fn i219(&self) -> bool {
        self.modelo == Modelo::I219
    }

    // --- El semáforo (§4.2 en el I219; el del MDIO en el 82574, §14.3) ---

    fn coger(&self) -> bool {
        if self.i219() {
            if !hasta(100, 1000, || reg32(EXTCNF_CTRL) & SWFLAG == 0) {
                println!("  [intel-red] el semáforo del PHY lo tiene otro");
                return false;
            }
            activar(EXTCNF_CTRL, SWFLAG);
            if !hasta(1000, 1000, || reg32(EXTCNF_CTRL) & SWFLAG != 0) {
                quitar(EXTCNF_CTRL, SWFLAG);
                println!("  [intel-red] el semáforo del PHY lo tiene el ME o el hardware");
                return false;
            }
            true
        } else {
            for _ in 0..10 {
                activar(EXTCNF_CTRL, SWFLAG);
                if reg32(EXTCNF_CTRL) & SWFLAG != 0 {
                    return true;
                }
                dashos::dormir_us(2000);
            }
            println!("  [intel-red] no se consigue el MDIO");
            false
        }
    }

    fn soltar(&self) {
        quitar(EXTCNF_CTRL, SWFLAG);
    }

    // --- El PHY (§4) ---

    /// Una transacción del MDIC (§4.1): leer (`v` = None) o escribir el registro `r` del PHY de dirección `a`.
    fn mdic_una(a: u32, r: u32, v: Option<u16>) -> Option<u16> {
        let orden = match v {
            Some(x) => x as u32 | 0x0400_0000,
            None => 0x0800_0000,
        };
        poner_reg32(MDIC, orden | (r << 16) | (a << 21));
        for _ in 0..1920 {
            girar_us(50);
            let m = reg32(MDIC);
            if m & (1 << 28) != 0 {
                if m & (1 << 30) != 0 || (m >> 16) & 0x1F != r {
                    return None;
                }
                return Some(m as u16);
            }
        }
        None
    }

    /// Con un reintento a los 10 ms (§4.1), salvo en los cambios de modo del enlace MAC-PHY (`reintentar` falso).
    fn mdic(&self, a: u32, r: u32, v: Option<u16>, reintentar: bool) -> Option<u16> {
        Self::mdic_una(a, r, v).or_else(|| {
            if reintentar && self.i219() {
                dormir_ms(10);
                Self::mdic_una(a, r, v)
            } else {
                None
            }
        })
    }

    /// Acceso al registro (página, registro) del PHY, con el semáforo ya cogido. I219: §4.3; 82574: §14.3.
    fn phy(&self, (pagina, r): (u16, u32), v: Option<u16>, reintentar: bool) -> Option<u16> {
        if self.i219() {
            let (pagina, dir) = match pagina {
                0 => (0, 2),
                768 => (0, 1),
                p if p > 768 && p < 800 => (p, 1),
                // Las páginas 1 a 767 y la 800 tienen un acceso especial que aquí no se usa (§4.3).
                _ => return None,
            };
            if r > 15 {
                self.mdic(1, 31, Some(pagina << 5), reintentar)?;
            }
            self.mdic(dir, r, v, reintentar)
        } else {
            if r > 15 {
                self.mdic(1, 22, Some(pagina), reintentar)?;
            }
            self.mdic(1, r, v, reintentar)
        }
    }
    fn phy_l(&self, reg: (u16, u32)) -> Option<u16> {
        self.phy(reg, None, true)
    }
    fn phy_e(&self, reg: (u16, u32), v: u16) -> bool {
        self.phy(reg, Some(v), true).is_some()
    }
    /// Leer, cambiar y escribir un registro del PHY (con el semáforo cogido).
    fn phy_cambiar(&self, reg: (u16, u32), f: impl Fn(u16) -> u16) -> bool {
        match self.phy_l(reg) {
            Some(v) => self.phy_e(reg, f(v)),
            None => false,
        }
    }
    /// Lo mismo cogiendo y soltando el semáforo.
    fn phy_leer(&self, reg: (u16, u32)) -> Option<u16> {
        if !self.coger() {
            return None;
        }
        let v = self.phy_l(reg);
        self.soltar();
        v
    }
    /// Registro ampliado (EMI, §4.3), con el semáforo cogido.
    fn emi_escribir(&self, dir: u16, v: u16) -> bool {
        self.phy_e(EMI_DIR, dir) && self.phy_e(EMI_DATO, v)
    }

    // --- El ME (§2.2, §6.3) ---

    fn fw_valido() -> bool {
        reg32(FWSM) & FWSM_FW_VALID != 0
    }

    /// ¿Bloquea el ME el reinicio del PHY? Se mira cada 10 ms hasta 30 veces antes de darlo por bloqueado (§6.3).
    fn mirar_bloqueo(&mut self) -> bool {
        self.bloqueado = !hasta(30, 10_000, || reg32(FWSM) & FWSM_RSPCIPHY != 0);
        self.bloqueado
    }

    // --- La NVM del I219 (§5) ---

    /// Una palabra doble de la flash, en la dirección de bytes `d`.
    fn flash32(d: u32) -> Option<u32> {
        for _ in 0..10 {
            let hs = reg32(HSFSTS);
            if hs & (1 << 14) == 0 {
                return None;
            }
            // Borrar FLCERR y DAEL (se borran con 1); esperar a que no haya un ciclo en marcha; borrar FLCDONE.
            poner_reg32(HSFSTS, (hs & 0xFFFF) | 0x6);
            if !hasta(1000, 100, || reg32(HSFSTS) & (1 << 5) == 0) {
                return None;
            }
            poner_reg32(HSFSTS, (reg32(HSFSTS) & 0xFFFF & !0x6) | 1);
            // Leer 4 bytes: FLDBCOUNT = 3, FLCYCLE = 0.
            let control: u32 = 3 << 8;
            poner_reg32(HSFSTS, control << 16);
            poner_reg32(FADDR, d & 0xFF_FFFF);
            poner_reg32(HSFSTS, (control | 1) << 16);
            // El código de Intel espera hasta 10 s; aquí, 100 ms (§13.2).
            if !hasta(1000, 100, || reg32(HSFSTS) & 1 != 0) {
                return None;
            }
            if reg32(HSFSTS) & (1 << 1) == 0 {
                return Some(reg32(FDATA0));
            }
        }
        None
    }

    /// Palabras de un banco de la NVM.
    fn palabras_banco() -> u32 {
        let tamano = (((reg32(STRAP) >> 1) & 0x1F) + 1) * 4096;
        tamano / 4
    }

    /// La palabra `w` de la NVM en el banco `banco`.
    fn nvm_palabra(banco: u32, w: u32) -> Option<u16> {
        let d = (banco * Self::palabras_banco() + w) * 2;
        let v = Self::flash32(d & !3)?;
        Some(if w & 1 == 0 { v as u16 } else { (v >> 16) as u16 })
    }

    /// El banco válido de la NVM: el que tiene `10` en los bits 7 y 6 del byte alto de su palabra 0x13 (§5).
    fn banco_valido() -> Option<u32> {
        (0..2).find(|&b| Self::nvm_palabra(b, 0x13).is_some_and(|v| (v >> 8) & 0xC0 == 0x80))
    }

    // --- Arranque del I219 (§6) ---

    /// Vaciar los anillos si el aparato lo pide (§13.1). `propio`: programar antes nuestro anillo de enviar (al
    /// arrancar, el de `TDBAL` es del sistema anterior).
    fn vaciar_anillos(&self, propio: bool) {
        activar(FEXTNVM11, 1 << 13);
        if cfg16(0xE4) & (1 << 8) == 0 || reg32(TDLEN) == 0 {
            return;
        }
        println!("  [intel-red] el aparato pide vaciar los anillos");
        if propio {
            poner_reg32(TDBAL, (self.bus + M_TX as u64) as u32);
            poner_reg32(TDBAH, ((self.bus + M_TX as u64) >> 32) as u32);
            poner_reg32(TDLEN, (N_TX * 16) as u32);
            poner_reg32(TDH, 0);
            poner_reg32(TDT, 0);
        }
        // El envío: un descriptor de 512 bytes sin EOP ni RS, que apunta al propio anillo.
        activar(TCTL, TCTL_EN);
        let i = reg32(TDT) as usize % N_TX;
        let d = M_TX + 16 * i;
        poner64(d, self.bus + M_TX as u64);
        poner32(d + 8, 512 | ((TX_IFCS as u32) << 24));
        poner32(d + 12, 0);
        dashos::barrera::total();
        poner_reg32(TDT, ((i + 1) % N_TX) as u32);
        girar_us(250);
        // La recepción, si sigue pidiéndolo.
        if cfg16(0xE4) & (1 << 8) != 0 {
            quitar(RCTL, RCTL_EN);
            vaciar();
            girar_us(150);
            let r = (reg32(RXDCTL) & 0xFFFF_C000) | 31 | (1 << 8) | (1 << 24);
            poner_reg32(RXDCTL, r);
            activar(RCTL, RCTL_EN);
            vaciar();
            girar_us(150);
            quitar(RCTL, RCTL_EN);
        }
    }

    /// Ciclo de energía LANPHYPC (§6.2, paso 3).
    fn ciclo_lanphypc() {
        poner_reg32(FEXTNVM3, (reg32(FEXTNVM3) & !(3 << 26)) | (2 << 26));
        poner_reg32(CTRL, (reg32(CTRL) | CTRL_LANPHYPC_OVERRIDE) & !CTRL_LANPHYPC_VALUE);
        vaciar();
        dormir_ms(1);
        quitar(CTRL, CTRL_LANPHYPC_OVERRIDE);
        vaciar();
        if !hasta(20, 5000, || reg32(CTRL_EXT) & EXT_LPCD != 0) {
            println!("  [intel-red] el ciclo de energía del PHY no termina");
        }
        dormir_ms(30);
    }

    /// ¿Responde el PHY? PHYID1 y PHYID2, hasta dos intentos, sin los reintentos del MDIC (§6.2, paso 3).
    fn phy_responde(&self) -> bool {
        (0..2).any(|_| {
            let a = self.phy(PHYID1, None, false);
            let b = self.phy(PHYID2, None, false);
            matches!((a, b), (Some(x), Some(y)) if x != 0xFFFF && y != 0xFFFF)
        })
    }

    /// Sacar el PHY del ULP y del modo SMBus (§6.2). Falso si el PHY no responde.
    fn despertar_phy(&mut self) -> bool {
        activar(EXTCNF_CTRL, GATE_PHY_CFG);
        self.con_me = Self::fw_valido();
        if self.con_me {
            poner_reg32(H2ME, (reg32(H2ME) & !(1 << 11)) | (1 << 12));
            if !hasta(250, 10_000, || reg32(FWSM) & FWSM_ULP_CFG_DONE == 0) {
                println!("  [intel-red] el ME no saca al PHY del reposo (ULP); se sigue");
            }
            quitar(H2ME, 1 << 12);
        } else if self.coger() {
            Self::ciclo_lanphypc();
            let mut smb = self.phy(CV_SMB_CTRL, None, false);
            if smb.is_none() {
                activar(CTRL_EXT, EXT_FORCE_SMBUS);
                dormir_ms(50);
                smb = self.phy(CV_SMB_CTRL, None, false);
            }
            if let Some(v) = smb {
                self.phy(CV_SMB_CTRL, Some(v & !1), false);
            }
            quitar(CTRL_EXT, EXT_FORCE_SMBUS);
            if let Some(v) = self.phy(HV_PM_CTRL, None, false) {
                self.phy(HV_PM_CTRL, Some(v | (1 << 14)), false);
            }
            if let Some(v) = self.phy(I218_ULP_CONFIG1, None, false) {
                let v = v & !((1 << 2) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 8) | (1 << 10) | (1 << 11) | (1 << 12));
                self.phy(I218_ULP_CONFIG1, Some(v), false);
                self.phy(I218_ULP_CONFIG1, Some(v | 1), false);
            }
            quitar(FEXTNVM7, 1 << 5);
            self.soltar();
            self.mirar_bloqueo();
            self.reiniciar_phy();
            dormir_ms(50);
        }
        if !self.coger() {
            return false;
        }
        let mut bien = self.phy_responde();
        if !bien {
            activar(CTRL_EXT, EXT_FORCE_SMBUS);
            dormir_ms(50);
            bien = self.phy_responde();
        }
        if !bien {
            if self.mirar_bloqueo() {
                println!("  [intel-red] el PHY no responde y el ME bloquea el ciclo de energía");
            } else {
                Self::ciclo_lanphypc();
                bien = self.phy_responde();
                if !bien {
                    quitar(CTRL_EXT, EXT_FORCE_SMBUS);
                    bien = self.phy_responde();
                }
                if !bien {
                    println!("  [intel-red] el PHY no responde");
                }
            }
        }
        // Sin ME, cada vez que el PHY responde (también si solo respondió forzando SMBus) se vuelve al modo PCIe. Con
        // ME no se quita nada: el ME decide el modo y puede dejar `FORCE_SMBUS` a 1 (§6.2, paso 3).
        if bien && !self.con_me {
            if let Some(v) = self.phy(CV_SMB_CTRL, None, false) {
                self.phy(CV_SMB_CTRL, Some(v & !1), false);
            }
            quitar(CTRL_EXT, EXT_FORCE_SMBUS);
        }
        self.soltar();
        if bien && !self.mirar_bloqueo() {
            self.reiniciar_phy();
            self.mirar_bloqueo();
        }
        bien
    }

    /// Reinicio solo del PHY (§6.3), si el ME no lo bloquea.
    fn reiniciar_phy(&mut self) {
        if self.bloqueado || !self.coger() {
            return;
        }
        activar(CTRL, CTRL_PHY_RST);
        vaciar();
        girar_us(100);
        quitar(CTRL, CTRL_PHY_RST);
        vaciar();
        girar_us(150);
        self.soltar();
        dormir_ms(10);
        self.tras_reiniciar_phy();
    }

    /// Después de reiniciar el PHY (§6.4).
    fn tras_reiniciar_phy(&self) {
        dormir_ms(10);
        if !hasta(1500, 100, || reg32(STATUS) & STATUS_LAN_INIT_DONE != 0) {
            println!("  [intel-red] el PHY no termina de configurarse; puede quedarse sin enlace");
        }
        quitar(STATUS, STATUS_LAN_INIT_DONE);
        if reg32(STATUS) & STATUS_PHYRA != 0 {
            quitar(STATUS, STATUS_PHYRA);
        }
        let banco = Self::banco_valido();
        if banco.is_none() {
            println!("  [intel-red] sin banco válido en la NVM");
        }
        if self.bloqueado {
            return;
        }
        dormir_ms(10);
        if !self.coger() {
            return;
        }
        self.phy_cambiar(BM_PORT_GEN_CFG, |v| v & !(1 << 4));
        let por_programa = reg32(FEXTNVM) & (1 << 27) != 0;
        let pares = (reg32(EXTCNF_SIZE) >> 16) & 0xFF;
        if por_programa && pares != 0 {
            // La configuración "LCD" desde la NVM (§6.4, paso 5).
            let strap = reg32(STRAP);
            let f = (strap >> 12) & 3;
            self.phy_cambiar(HV_SMB_ADDR, |v| {
                let mut v = (v & !0x7F) | ((strap >> 17) & 0x7F) as u16 | (1 << 9) | (1 << 7);
                if f > 0 {
                    let f = f - 1;
                    v = (v & !0x1100) | ((f & 1) << 8) as u16 | (((f >> 1) & 1) << 12) as u16;
                }
                v
            });
            if f == 0 {
                println!("  [intel-red] frecuencia de SMBus no admitida");
            }
            self.phy_e(HV_LED_CONFIG, reg32(LEDCTL) as u16);
            let inicio = ((reg32(EXTCNF_CTRL) >> 16) & 0xFFF) * 2;
            let banco = banco.unwrap_or(0);
            let mut pagina = 0u32;
            for i in 0..pares {
                let (Some(dato), Some(registro)) =
                    (Self::nvm_palabra(banco, inicio + 2 * i), Self::nvm_palabra(banco, inicio + 2 * i + 1))
                else {
                    println!("  [intel-red] no se lee la configuración del PHY de la NVM");
                    break;
                };
                if registro as u32 & 0x1F == 31 {
                    pagina = dato as u32;
                    continue;
                }
                let plano = (registro as u32 & 0x1F) | pagina;
                self.phy(((plano >> 5) as u16, plano & 0x1F), Some(dato), true);
            }
        }
        if por_programa {
            // Los bits OEM (§6.4, paso 6).
            let pc = reg32(PHY_CTRL);
            self.phy_cambiar(HV_OEM_BITS, |v| {
                let mut v = v & !((1 << 6) | (1 << 2));
                if pc & (1 << 6) != 0 {
                    v |= 1 << 6;
                }
                if pc & (1 << 1) != 0 {
                    v |= 1 << 2;
                }
                v | (1 << 10)
            });
        }
        self.soltar();
    }

    /// Quitar el maestro de bus de la MAC (§6.3, paso 2).
    fn sin_maestro() {
        activar(CTRL, CTRL_GIO_MASTER_DISABLE);
        if !hasta(800, 100, || reg32(STATUS) & STATUS_GIO_MASTER_ENABLE == 0) {
            println!("  [intel-red] la MAC no suelta el bus; se sigue");
        }
    }

    /// Reinicio global de la MAC (y del PHY si se puede): §6.3 en el I219, §14.3 en el 82574. `tras_phy`: hacer lo de
    /// después de reiniciar el PHY (§6.4); en la parada final, no (§12, paso 6).
    fn reiniciar(&mut self, tras_phy: bool) {
        // El reparto del búfer interno se escribe antes: el reinicio lo conserva (§6.5, punto 8; §14.3).
        poner_reg32(PBA, if self.i219() { 26 } else { 32 });
        if self.i219() {
            self.vaciar_anillos(false);
            Self::sin_maestro();
            poner_reg32(IMC, u32::MAX);
            poner_reg32(RCTL, 0);
            poner_reg32(TCTL, TCTL_PSP);
            vaciar();
            dormir_ms(10);
            let mut ctrl = reg32(CTRL);
            let con_phy = !self.mirar_bloqueo();
            if con_phy {
                ctrl |= CTRL_PHY_RST;
            }
            // No se suelta: el reinicio lo borra (§6.3, paso 5).
            self.coger();
            // Que la lectura de configuración termine antes del reinicio (§6.3, pasos 6 y 8).
            poner_reg32(STRAP, cfg16(0) as u32);
            poner_reg32(CTRL, ctrl | CTRL_RST);
            // Ni una lectura de registros en 20 ms.
            dormir_ms(20);
            poner_reg32(STRAP, cfg16(0) as u32);
            if con_phy && tras_phy {
                self.tras_reiniciar_phy();
            }
            poner_reg32(IMC, u32::MAX);
            reg32(ICR);
            activar(KABGTXD, 0x0005_0000);
            if reg32(CTRL_EXT) & EXT_FORCE_SMBUS != 0 {
                // Con el ME activo no es un fallo nuestro (§6.3, notas del paso 12).
                println!(
                    "  [intel-red] el enlace con el PHY sigue forzado a SMBus tras el reinicio ({}); se sigue",
                    if self.con_me { "lo deja el ME" } else { "sin ME" }
                );
            }
        } else {
            Self::sin_maestro();
            poner_reg32(IMC, u32::MAX);
            poner_reg32(RCTL, 0);
            quitar(TCTL, TCTL_EN);
            vaciar();
            dormir_ms(10);
            let mdio = self.coger();
            activar(CTRL, CTRL_RST);
            if mdio {
                self.soltar();
            }
            if !hasta(10, 1000, || reg32(EECD) & (1 << 9) != 0) {
                println!("  [intel-red] la EEPROM no termina de leerse");
            }
            dormir_ms(25);
            poner_reg32(IMC, u32::MAX);
            reg32(ICR);
        }
        // El reinicio borra CTRL_EXT: decir otra vez al firmware que hay controlador (§6.3, paso 12).
        activar(CTRL_EXT, EXT_DRV_LOAD);
    }

    /// Los bits obligatorios de la MAC (§6.5 en el I219; §14.3 en el 82574).
    fn bits_obligatorios(&self) {
        if self.i219() {
            activar(CTRL_EXT, EXT_BIT22 | EXT_PHYPDEN);
            activar(TXDCTL, 1 << 22);
            activar(TXDCTL1, 1 << 22);
            activar(TARC0, (1 << 23) | (1 << 24) | (1 << 26) | (1 << 27));
            let mut t1 = reg32(TARC1) | (1 << 24) | (1 << 26) | (1 << 30);
            if reg32(TCTL) & TCTL_MULR != 0 {
                t1 &= !(1 << 28);
            } else {
                t1 |= 1 << 28;
            }
            poner_reg32(TARC1, t1);
            poner_reg32(RFCTL, (reg32(RFCTL) | (1 << 6) | (1 << 7)) & !(1 << 15));
            activar(PBECCSTS, 1 << 16);
            activar(CTRL, CTRL_MEHE);
            // Umbrales de envío (§6.5, punto 6): WTHRESH 1, GRAN 1, PTHRESH 31.
            poner_reg32(TXDCTL, (reg32(TXDCTL) & !(0x003F_0000 | 0x3F)) | 0x0101_001F);
        } else {
            activar(TXDCTL, 1 << 22);
            activar(TXDCTL1, 1 << 22);
            poner_reg32(TARC0, (reg32(TARC0) & !(0xF << 27)) | (1 << 26));
            quitar(CTRL, 1 << 29);
            poner_reg32(CTRL_EXT, (reg32(CTRL_EXT) & !(1 << 23)) | (1 << 22));
            poner_reg32(GCR, (reg32(GCR) | (1 << 22)) & !(1 << 27));
            activar(GCR2, 1);
            quitar(RFCTL, 1 << 15);
            // Sin ASPM (L0s ni L1) en el control del enlace del aparato.
            if let Some(c) = capacidad(0x10) {
                poner_cfg16(c + 0x10, cfg16(c + 0x10) & !3);
            }
            // WTHRESH 1, GRAN 1 y COUNT_DESC, sin tocar PTHRESH (§14.3).
            poner_reg32(TXDCTL, (reg32(TXDCTL) & !0x003F_0000) | 0x0141_0000);
        }
        // Las dos colas, iguales.
        poner_reg32(TXDCTL1, reg32(TXDCTL));
        activar(CTRL_EXT, EXT_RO_DIS);
    }

    /// La negociación del enlace (§6.6).
    fn enlace(&self) {
        poner_reg32(CTRL, (reg32(CTRL) & !(CTRL_RFCE | CTRL_TFCE | CTRL_FRCSPD | CTRL_FRCDPX)) | CTRL_SLU);
        if (self.i219() && self.bloqueado) || !self.coger() {
            return;
        }
        if self.i219() {
            self.phy_cambiar(I82577_CFG, |v| v | (1 << 15) | (1 << 11) | (1 << 10));
            self.phy_cambiar(I82577_PHY_CTRL_2, |v| (v & !(3 << 9)) | (2 << 9));
        } else {
            self.phy_82574();
        }
        self.phy_e(ANAR, 0x01E1);
        self.phy_cambiar(GBCR, |v| v | (1 << 9));
        self.phy_cambiar(BMCR, |v| v | (1 << 12) | (1 << 9));
        self.soltar();
    }

    /// La configuración propia del PHY del 82574 (§14.3, "Configuración del PHY"), con el semáforo cogido.
    fn phy_82574(&self) {
        let de_qemu = self.phy_l(PHYID1) == Some(0x0141) && self.phy_l(PHYID2) == Some(0x0CB1);
        let confirmar = || {
            self.phy_cambiar(BMCR, |v| v | (1 << 15));
            girar_us(1);
        };
        let Some(v) = self.phy_l(PHY_ESPECIFICO) else { return };
        // MDI/MDI-X automático; corrección de polaridad automática encendida.
        let mut v = ((v & !(3 << 5)) | (3 << 5)) & !(1 << 1);
        if de_qemu {
            v &= !(1 << 11);
            self.phy_e(PHY_ESPECIFICO, v);
            confirmar();
        }
        v |= 1 << 11;
        self.phy_e(PHY_ESPECIFICO, v);
        if de_qemu {
            self.phy_e((0, 29), 0x0003);
            self.phy_e((0, 30), 0x0000);
        }
        confirmar();
    }

    /// Al subir el enlace en el I219 (§10, pasos 1 a 3 y 5); el 4 va en cada comprobación.
    fn enlace_arriba_i219(&self, velocidad: u32, duplex: bool) {
        let diez_semi = velocidad == 0 && !duplex;
        let campo = if diez_semi {
            0xFF
        } else if duplex && velocidad != 2 {
            0x0C
        } else {
            0x08
        };
        poner_reg32(TIPG, (reg32(TIPG) & !0x3FF) | campo);
        if !self.coger() {
            return;
        }
        self.emi_escribir(I217_RX_CONFIG, if diez_semi { 0 } else { 1 });
        let gig = velocidad == 2;
        self.phy_cambiar(I217_PLL_CLOCK_GATE, |v| (v & !0x7FF) | if gig { 0xFA } else { 0x3E8 });
        if gig {
            self.phy_cambiar(HV_PM_CTRL, |v| v | (1 << 9));
            self.phy_cambiar(P776_20, |v| if (v >> 2) & 0x3FF < 0x18 { (v & !(0x3FF << 2)) | (0x18 << 2) } else { v });
        } else {
            self.phy_e(P776_20, 0xC023);
        }
        self.phy_cambiar(I82579_LPI_CTRL, |v| v & !((1 << 13) | (1 << 14)));
        self.soltar();
    }
}

impl Tarjeta {
    /// La parada ordenada (§12). Con `final_`, también el reinicio global, el PHY apagado (si el ME no lo usa), el
    /// mando devuelto al firmware y el maestro de bus quitado: después solo queda soltar la DMA. Sin él (para volver
    /// a arrancar en marcha, §14.3), solo los pasos 1 a 5: la DMA y el canal siguen.
    fn parar(&mut self, msix: bool, final_: bool) {
        // Un aparato colgado lee todo a unos: no hay nada más que hacer sin un reinicio PCI (§12, paso 10).
        if reg32(STATUS) != u32::MAX {
            // En el 82574, la recepción no se apaga nunca (§14.3).
            if self.i219() {
                quitar(RCTL, RCTL_EN);
            }
            quitar(TCTL, TCTL_EN);
            vaciar();
            dormir_ms(10);
            poner_reg32(IMC, u32::MAX);
            if msix {
                poner_reg32(EIAC, 0);
            }
            vaciar();
            poner_reg32(ICR, reg32(ICR));
            if self.i219() {
                self.vaciar_anillos(false);
            }
            if !final_ {
                return;
            }
            self.reiniciar(false);
            // El PHY, apagado, salvo si el ME lo usa (§12, paso 7).
            if (!self.i219() || (!self.con_me && !self.bloqueado)) && self.coger() {
                self.phy_cambiar(BMCR, |v| v | (1 << 11));
                self.soltar();
                dormir_ms(1);
            }
            // El mando, al firmware, después del reinicio (§12, paso 8).
            quitar(CTRL_EXT, EXT_DRV_LOAD);
        } else if !final_ {
            return;
        }
        // Sin maestro de bus en el PCI (§12, paso 9).
        poner_cfg16(4, cfg16(4) & !(1 << 2));
    }
}

/// Lo que tiró la tarjeta desde la última vez (§11.1): (cortas, largas, sin sitio). Leerlos los borra.
fn contadores() -> (u32, u32, u32) {
    (reg32(RUC), reg32(ROC).wrapping_add(reg32(RJC)), reg32(MPC))
}

/// Todo lo de después del reinicio global (§6.5 a §6.8; §14.3 y §14.4 en el 82574): bits obligatorios, la MAC, las
/// tablas de direcciones, el enlace, los anillos (los descriptores, rehechos) y la configuración de las
/// interrupciones. Devuelve la máscara de interrupciones, que se pone después en `IMS`.
fn programar(t: &Tarjeta, mac: &[u8; 6], msix: bool) -> u32 {
    let bus = t.bus;
    t.bits_obligatorios();
    contadores();
    // La MAC (§9), válida.
    poner_reg32(RAL0, u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]));
    vaciar();
    poner_reg32(RAH0, mac[4] as u32 | (mac[5] as u32) << 8 | 1 << 31);
    vaciar();
    // La multidifusión, entera por MPE: la tabla a cero (§11). En el I219, las entradas 1 a 11 son las compartidas con
    // el ME (`SHRA`) y no se tocan (§6.5, punto 10); en el 82574, las otras 14 a cero.
    for i in 0..if t.i219() { 32 } else { 128 } {
        poner_reg32(MTA + 4 * i, 0);
    }
    if !t.i219() {
        for i in 1..=14 {
            poner_reg32(RAL0 + 8 * i, 0);
            poner_reg32(RAH0 + 8 * i, 0);
        }
    }
    t.enlace();
    // Enviar (§6.7).
    for i in 0..N_TX {
        poner64(M_TX + 16 * i, 0);
        poner64(M_TX + 16 * i + 8, 0);
    }
    poner_reg32(TDBAL, (bus + M_TX as u64) as u32);
    poner_reg32(TDBAH, ((bus + M_TX as u64) >> 32) as u32);
    poner_reg32(TDLEN, (N_TX * 16) as u32);
    poner_reg32(TDH, 0);
    poner_reg32(TDT, 0);
    poner_reg32(TIDV, 0);
    poner_reg32(TADV, 0);
    poner_reg32(TIPG, 0x0060_2008);
    poner_reg32(TCTL, (reg32(TCTL) & !TCTL_CT) | TCTL_EN | TCTL_PSP | TCTL_RTLC | TCTL_MULR | (15 << 4) | (63 << 12));
    // Recibir.
    quitar(RCTL, RCTL_EN);
    for i in 0..N_RX {
        poner64(M_RX + 16 * i, bus + (M_RX_BUF + i * BUFER) as u64);
        poner64(M_RX + 16 * i + 8, 0);
    }
    poner_reg32(RDBAL, (bus + M_RX as u64) as u32);
    poner_reg32(RDBAH, ((bus + M_RX as u64) >> 32) as u32);
    poner_reg32(RDLEN, (N_RX * 16) as u32);
    poner_reg32(RDH, 0);
    poner_reg32(RDT, 0);
    poner_reg32(RXCSUM, 0);
    poner_reg32(RDTR, 0);
    poner_reg32(RADV, 0);
    poner_reg32(ITR, 0);
    dashos::barrera::total();
    poner_reg32(RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC | RCTL_MPE);
    poner_reg32(RDT, (N_RX - 1) as u32);
    // Las interrupciones (§6.8; §14.4 con la MSI-X del 82574).
    if msix && !t.i219() {
        poner_reg32(IVAR, 0x8008_0808);
        poner_reg32(CTRL_EXT, (reg32(CTRL_EXT) & !EXT_IAME) | EXT_PBA_CLR | EXT_EIAME);
        poner_reg32(EIAC, 0);
        poner_reg32(EITR0, 0);
        activar(RFCTL, 1 << 12);
        (1 << 20) | (1 << 22) | (1 << 24) | I_LSC | (1 << 6)
    } else {
        0x9D | if t.i219() { 1 << 22 } else { 0 }
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

/// Qué es el aparato, por su ID (§2.1, §14.1).
fn modelo(id: u32) -> Option<Modelo> {
    if id & 0xFFFF != 0x8086 {
        return None;
    }
    match id >> 16 {
        0x10D3 | 0x10F6 => Some(Modelo::M82574),
        0x15BB..=0x15BE | 0x15DF..=0x15E2 | 0x0D4C..=0x0D4F => Some(Modelo::I219),
        _ => None,
    }
}

/// La MAC de `RAL0`/`RAH0` (§9); en el 82574, si está a cero, de la EEPROM (`EERD`, §14.3).
fn leer_mac(t: &Tarjeta) -> [u8; 6] {
    let (l, h) = (reg32(RAL0), reg32(RAH0));
    let mut mac = [l as u8, (l >> 8) as u8, (l >> 16) as u8, (l >> 24) as u8, h as u8, (h >> 8) as u8];
    if mac == [0; 6] && !t.i219() {
        for w in 0..3u32 {
            poner_reg32(EERD, (w << 2) | 1);
            if hasta(100, 100, || reg32(EERD) & 2 != 0) {
                let v = reg32(EERD) >> 16;
                mac[2 * w as usize] = v as u8;
                mac[2 * w as usize + 1] = (v >> 8) as u8;
            }
        }
    }
    mac
}

fn principal() -> u64 {
    // Con llave 5, la 4 es la interrupción y la 5 el canal "red"; si no, el canal es la 4.
    let existe = |k: u32| {
        let copia = dashos::llave_duplicar(k, dashos::LEER, 0);
        if copia > 0 {
            dashos::cerrar(copia as u32);
        }
        copia > 0
    };
    let (irq, servicio) = if existe(5) { (Some(4u32), 5u32) } else { (None, 4u32) };
    if dashos::registros_proyectar(2, VA) != 0 || dashos::registros_proyectar(3, VA_CFG) != 0 {
        println!("  [intel-red] sin registros");
        return 1;
    }
    let id = cfg32(0);
    let Some(m) = modelo(id) else {
        println!("  [intel-red] aparato {:04x}:{:04x} sin soporte todavía", id & 0xFFFF, id >> 16);
        return 2;
    };
    // La memoria DMA, antes de reiniciar: el vaciado del I219 (§13.1) usa nuestro anillo.
    let (mem_dma, bus) = dashos::memoria_dma(1, TAM_DMA);
    if mem_dma <= 0 || dashos::memoria_proyectar(mem_dma as u32, VA_DMA, true) != 0 {
        println!("  [intel-red] sin memoria DMA");
        return 3;
    }
    for i in (0..TAM_DMA).step_by(4) {
        poner32(i, 0);
    }
    let mut t = Tarjeta { modelo: m, con_me: false, bloqueado: false, bus };
    poner_reg32(IMC, u32::MAX);
    activar(CTRL_EXT, EXT_DRV_LOAD);
    if t.i219() {
        t.vaciar_anillos(true);
        if !t.despertar_phy() {
            println!("  [intel-red] se sigue sin el PHY: puede no haber enlace");
        }
    }
    t.reiniciar(true);
    // El aparato ya está reiniciado: la DMA de una vida anterior se puede soltar.
    dashos::dma_liberar(1);
    // La MAC (§9): la que dejó el reinicio en RAL0/RAH0 (o la EEPROM del 82574).
    let mut mac = leer_mac(&t);
    if mac == [0; 6] || mac[0] & 1 != 0 {
        println!("  [intel-red] sin dirección MAC; se usa una local");
        mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    }
    let msix = capacidad(0x11).is_some_and(|c| cfg32(c) & (1 << 31) != 0);
    let mascara = programar(&t, &mac, msix);
    // La memoria de las tramas, para el servicio.
    let mem = dashos::memoria_crear(TAM_TRAMAS);
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_TRAMAS, true) != 0 {
        println!("  [intel-red] sin memoria para las tramas");
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
        println!("  [intel-red] el servicio no recibe la tarjeta");
        return 5;
    }
    let irq = irq.filter(|&i| {
        let marcada = dashos::llave_duplicar(escribe, dashos::ESCRIBIR, M_IRQ);
        marcada > 0 && dashos::interrupcion_asociar(i, marcada as u32) == 0
    });
    reg32(ICR);
    poner_reg32(IMS, mascara);
    println!(
        "  [intel-red] {} ({:04x}:{:04x}); MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}; colas de {} y {}; avisos {}",
        if t.i219() { "I219" } else { "82574L" },
        id & 0xFFFF, id >> 16, mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], N_RX, N_TX,
        match irq {
            Some(_) if msix => "por su interrupción (MSI-X)",
            Some(_) => "por su interrupción (MSI)",
            None => "con un temporizador",
        }
    );
    if t.i219() {
        println!(
            "  [intel-red] ME: {}; reinicio del PHY {}",
            if t.con_me { "en marcha" } else { "no" },
            if t.bloqueado { "bloqueado por el ME" } else { "libre" }
        );
    }

    // El estado de los anillos: el siguiente de recibir que mirar, el siguiente de enviar libre y el más antiguo en
    // vuelo. Y si se está tirando una trama partida (sin EOP).
    let mut rx_sig = 0usize;
    let mut tx_cola = 0usize;
    let mut tx_limpio = 0usize;
    let mut partida = false;
    let mut con_enlace = false;
    let mut proxima_mirada = 0u64;
    let mut cuelgues = 0u32;
    let mut reiniciar_en_marcha = false;
    let mut rotas = 0u64;
    let mut buf = [0u8; 16];
    let mut ll = [0u32; 8];
    loop {
        let plazo = if irq.is_some() { 100_000 } else { 2_000 };
        let (n, k, marca) = dashos::canal_leer_plazo(lee, &mut buf, &mut ll, plazo);
        let k = if n >= 0 { k.min(ll.len()) } else { 0 };
        ll[..k].iter().for_each(|&l| {
            dashos::cerrar(l);
        });
        // Las causas: se leen y se borran escribiéndolas (borran con 1, §6.8).
        let causas = reg32(ICR);
        if causas != 0 {
            poner_reg32(ICR, causas);
        }
        if n >= 0 && marca == M_IRQ {
            if let Some(i) = irq {
                dashos::interrupcion_terminar(i);
            }
        }
        // El enlace: al cambiar (LSC) o cada segundo (§10).
        let ahora = dashos::ahora_us();
        if causas & I_LSC != 0 || ahora >= proxima_mirada {
            proxima_mirada = ahora + 1_000_000;
            let phy = t.phy_leer(BMSR).and_then(|_| t.phy_leer(BMSR));
            let st = reg32(STATUS);
            let arriba = st & STATUS_LU != 0 && phy.is_none_or(|b| b & (1 << 2) != 0);
            let (velocidad, duplex) = ((st >> 6) & 3, st & STATUS_FD != 0);
            if arriba != con_enlace {
                con_enlace = arriba;
                if arriba {
                    let mbs = match velocidad {
                        0 => 10,
                        1 => 100,
                        _ => 1000,
                    };
                    println!("  [intel-red] enlace arriba: {} Mb/s, {}", mbs, if duplex { "dúplex" } else { "semidúplex" });
                    if t.i219() {
                        t.enlace_arriba_i219(velocidad, duplex);
                    }
                } else {
                    println!("  [intel-red] enlace abajo");
                }
            }
            // Lo que tiró la tarjeta misma (§11.1), a los contadores del servicio.
            let (cortas, largas, sin_sitio) = contadores();
            for (i, n) in [(p::T_CORTAS, cortas), (p::T_LARGAS, largas), (p::T_SIN_SITIO, sin_sitio)] {
                if n != 0 {
                    poner_tramas(i, tramas(i).wrapping_add(n));
                }
            }
            if t.i219() {
                poner_reg32(FEXTNVM4, (reg32(FEXTNVM4) & !7) | 7);
            } else {
                // El cuelgue del PHY del 82574 (§14.3): el 21 de la página 0 a 0xFFFF y, solo entonces, el 10 con su
                // byte bajo a 0xFF; dos veces seguidas.
                let colgado = t.coger() && {
                    let r = t.phy_l((0, 21)) == Some(0xFFFF) && t.phy_l((0, 10)).is_some_and(|v| v & 0xFF == 0xFF);
                    t.soltar();
                    r
                };
                cuelgues = if colgado { cuelgues + 1 } else { 0 };
                if cuelgues == 2 {
                    cuelgues = 0;
                    reiniciar_en_marcha = true;
                }
            }
        }
        let mut movido = false;
        if reiniciar_en_marcha {
            // Parar y volver a arrancar entero, con la misma DMA y el mismo canal (§14.3): lo que estaba en vuelo
            // para enviar se pierde.
            reiniciar_en_marcha = false;
            let perdidas = (tx_cola + N_TX - tx_limpio) % N_TX;
            println!("  [intel-red] el PHY está colgado; se reinicia la tarjeta ({} tramas de enviar perdidas)", perdidas);
            t.parar(msix, false);
            t.reiniciar(true);
            let m = programar(&t, &mac, msix);
            reg32(ICR);
            poner_reg32(IMS, m);
            (rx_sig, tx_cola, tx_limpio, partida, con_enlace) = (0, 0, 0, false, false);
            movido = true;
        }
        // Lo recibido (§7.2): al anillo del servicio, si cabe y tiene buena medida.
        let mut ultimo = None;
        for _ in 0..N_RX {
            let d = M_RX + 16 * rx_sig;
            let estado = dma8(d + 12);
            if estado & DD == 0 {
                break;
            }
            dashos::barrera::lecturas();
            let largo = dma16(d + 8) as usize;
            let errores = dma8(d + 13);
            sumar(p::T_RECIBIDAS);
            if partida || estado & RX_EOP == 0 {
                // Una trama partida (más larga que un búfer) se tira entera, hasta la que lleva EOP.
                partida = estado & RX_EOP == 0;
                sumar(p::T_LARGAS);
            } else if errores != 0 {
                // Con error (CRC, símbolo, secuencia...): se tira; no hay contador `T_*` para esto.
                rotas += 1;
                if rotas == 1 {
                    println!("  [intel-red] llega una trama con error ({:#x}); se tiran", errores);
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
            // Devolver el descriptor: la dirección se queda; lo demás, a cero.
            poner32(d + 8, 0);
            poner32(d + 12, 0);
            ultimo = Some(rx_sig);
            rx_sig = (rx_sig + 1) % N_RX;
        }
        if let Some(u) = ultimo {
            dashos::barrera::total();
            poner_reg32(RDT, u as u32);
        }
        // Lo enviado (§7.1): libre desde el más antiguo mientras tenga DD.
        while tx_limpio != tx_cola && dma8(M_TX + 16 * tx_limpio + 12) & DD != 0 {
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
                let d = M_TX + 16 * tx_cola;
                poner64(d, bus + dest as u64);
                poner32(d + 8, largo as u32 | ((TX_EOP | TX_IFCS | TX_RS) as u32) << 24);
                poner32(d + 12, 0);
                tx_cola = (tx_cola + 1) % N_TX;
                sumar(p::T_ENVIADAS);
                puestos = true;
            } else {
                sumar(p::T_TX_MALAS);
            }
            poner_tramas(p::T_TX_LEIDAS, leidas.wrapping_add(1));
            movido = true;
        }
        if puestos {
            dashos::barrera::total();
            poner_reg32(TDT, tx_cola as u32);
        }
        if movido {
            // Si la cola del servicio está llena, se pierde: el servicio mira las ranuras en cada vuelta. Si el
            // servicio ya no está, la parada ordenada (§12) y fuera.
            if dashos::canal_escribir(servicio, &p::TRAMAS.to_le_bytes(), &[]) == dashos::E_CERRADO {
                println!("  [intel-red] el servicio de red se fue; se para la tarjeta");
                t.parar(msix, true);
                return 0;
            }
        }
    }
}
