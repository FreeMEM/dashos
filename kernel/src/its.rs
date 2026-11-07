//! El ITS del GICv3 (U6h): convierte las MSI de los aparatos del PCIe en LPI. Es lo que llevan las máquinas con GICv3
//! (los portátiles con Snapdragon, los servidores, QEMU `virt` con `gic-version=3`), que no tienen GICv2m.
//!
//! Escrito con la especificación de la arquitectura GICv3 y GICv4 de ARM (IHI 0069): LPI (cap. 5.1), el ITS (cap.
//! 5.2: tablas, cola de órdenes y las órdenes MAPD, MAPC, MAPTI y SYNC, 5.3) y sus registros (cap. 12.19).
//!
//! Lo mínimo para dar una LPI a cada aparato: tablas planas de aparatos y de colecciones de una o pocas páginas, una
//! colección (la 0) que lleva al núcleo 0 (donde van todas las interrupciones de aparatos, C20), y una tabla de
//! traducción (ITT) de dos sucesos por aparato. El número de aparato (DeviceID) es el RID del PCIe (bus, aparato y
//! función), como en QEMU `virt` (`msi-map` de identidad); el `msi-map` del árbol o el IORT de ACPI no se leen aún.

use crate::cerrojo::Cerrojo;
use crate::dtb::Dtb;
use crate::kprintln;
use crate::memoria;
use crate::mmio::RegLe32;

const GITS_CTLR: usize = 0x0000;
const GITS_TYPER: usize = 0x0008;
const GITS_CBASER: usize = 0x0080;
const GITS_CWRITER: usize = 0x0088;
const GITS_CREADR: usize = 0x0090;
const GITS_BASER: usize = 0x0100;
/// Donde escribe el aparato el número de suceso (en el segundo marco de 64 KB).
const GITS_TRANSLATER: u64 = 0x1_0040;

const CTLR_ENCENDIDO: u32 = 1;
const CTLR_QUIETO: u32 = 1 << 31;

/// Las LPI que se reparten: desde la 8192, tantas como caben en las del kernel (`interrupciones::LPIS`).
pub const PRIMERA_LPI: u32 = 8192;
/// Bits de número de interrupción: hasta la 16383. La tabla de configuración ocupa 8 KB.
const IDBITS: u32 = 14;
/// Prioridad de las LPI (como las SPI de los controladores); el bit 0 la enciende.
const CONFIG_LPI: u8 = 0xa0;
const TAM_ITT: u64 = 256;
const APARATOS_ITT: u64 = memoria::PAGINA / TAM_ITT;

struct Its {
    base: usize,
    config: u64,
    fisica: u64,
    cola: u64,
    escrita: u64,
    itt: u64,
    aparatos: u64,
    siguiente: u32,
    rd_destino: u64,
}

static ITS: Cerrojo<Option<Its>> = Cerrojo::new(None);

impl Its {
    fn r(&self, off: usize) -> RegLe32 {
        RegLe32::new(self.base + off)
    }

    fn escribir64(&self, off: usize, v: u64) {
        self.r(off).escribir(v as u32);
        self.r(off + 4).escribir((v >> 32) as u32);
    }

    /// Pone una orden de 4 palabras de 64 bits en la cola y espera a que el ITS la lea.
    fn orden(&mut self, palabras: [u64; 4]) -> bool {
        let mut b = [0u8; 32];
        for (i, p) in palabras.iter().enumerate() {
            b[8 * i..8 * i + 8].copy_from_slice(&p.to_le_bytes());
        }
        memoria::escribir_fisica(self.cola + self.escrita, &b);
        self.escrita = (self.escrita + 32) % memoria::PAGINA;
        self.r(GITS_CWRITER).escribir(self.escrita as u32);
        for _ in 0..1_000_000 {
            let leida = self.r(GITS_CREADR).leer();
            if leida & 1 != 0 {
                kprintln!("its: la cola de órdenes se paró (CREADR {:#x})", leida);
                return false;
            }
            if leida as u64 & 0xf_ffe0 == self.escrita {
                return true;
            }
        }
        kprintln!("its: la orden no se lee");
        false
    }

    fn sync(&mut self) -> bool {
        self.orden([0x05, 0, self.rd_destino << 16, 0])
    }
}

/// Busca el ITS en el árbol ("arm,gic-v3-its"), enciende las LPI del núcleo 0 y el ITS. Verdad si queda listo.
pub fn iniciar(d: &Dtb) -> bool {
    if !crate::gic::es_v3() {
        return false;
    }
    let Some(n) = d.compatible("arm,gic-v3-its") else { return false };
    let Some((fisica, _)) = d.reg_cpu(&n, 0) else { return false };
    if !crate::placa::es_periferico(fisica) || !crate::placa::es_periferico(fisica + GITS_TRANSLATER) {
        return false;
    }
    // Las tablas de las LPI: configuración (un byte por LPI) y pendientes (un bit, alineada a 64 KB).
    let tam_config = (1usize << IDBITS) - PRIMERA_LPI as usize;
    let Some(config) = memoria::paginas_fijas(tam_config / memoria::PAGINA as usize, 1) else { return false };
    let Some(pendientes) = memoria::paginas_fijas(1, 16) else { return false };
    let lpis = [CONFIG_LPI; crate::interrupciones::LPIS as usize];
    memoria::escribir_fisica(config, &lpis);
    let Some((rd_fisica, procesador)) = crate::gic::encender_lpis(config, pendientes, IDBITS) else {
        kprintln!("its: el redistribuidor no tiene LPI");
        return false;
    };
    let base = crate::mmu::virt(fisica as usize);
    let mut its = Its { base, config, fisica, cola: 0, escrita: 0, itt: 0, aparatos: 0, siguiente: PRIMERA_LPI, rd_destino: 0 };
    // Apagado y quieto antes de tocar sus tablas.
    its.r(GITS_CTLR).escribir(0);
    for _ in 0..1_000_000 {
        if its.r(GITS_CTLR).leer() & CTLR_QUIETO != 0 {
            break;
        }
    }
    let typer = its.r(GITS_TYPER).leer();
    // PTA (bit 19): la colección apunta al redistribuidor por su dirección física (en unidades de 64 KB) o por su
    // número de procesador.
    its.rd_destino = if typer & (1 << 19) != 0 { rd_fisica >> 16 } else { procesador as u64 };
    // Tablas de aparatos (tipo 1) y de colecciones (tipo 4): planas, de páginas de 4 KB.
    for i in 0..8 {
        let off = GITS_BASER + 8 * i;
        let v = its.r(off).leer() as u64 | (its.r(off + 4).leer() as u64) << 32;
        let tipo = (v >> 56) & 7;
        if tipo != 1 && tipo != 4 {
            continue;
        }
        let paginas = if tipo == 1 { 4 } else { 1 };
        let Some(t) = memoria::paginas_fijas(paginas, 1) else { return false };
        let nuevo = (1 << 63) | (0b111 << 59) | (tipo << 56) | (v & (0x1f << 48)) | t | (0b01 << 10) | (paginas as u64 - 1);
        its.escribir64(off, nuevo);
    }
    let Some(cola) = memoria::paginas_fijas(1, 1) else { return false };
    its.cola = cola;
    its.escribir64(GITS_CBASER, (1 << 63) | (0b111 << 59) | cola | (0b01 << 10));
    its.r(GITS_CWRITER).escribir(0);
    let Some(itt) = memoria::paginas_fijas(1, 1) else { return false };
    its.itt = itt;
    its.r(GITS_CTLR).escribir(CTLR_ENCENDIDO);
    // La colección 0, al núcleo 0 (MAPC: ICID en la palabra 2, destino en sus bits 50:16, válida).
    let destino = its.rd_destino;
    if !its.orden([0x09, 0, (1 << 63) | (destino << 16), 0]) || !its.sync() {
        return false;
    }
    kprintln!("its: en {:#x}, LPI desde la {}", fisica, PRIMERA_LPI);
    *ITS.tomar() = Some(its);
    true
}

/// Enciende o apaga una LPI en la tabla de configuración y hace que el ITS la vuelva a leer (INVALL de la colección
/// 0 y SYNC). Se enciende al asociarla a un canal y se apaga al soltarla: hasta entonces, nadie la atiende.
pub fn encender(lpi: u32, si: bool) {
    let mut g = ITS.tomar();
    let Some(its) = g.as_mut() else { return };
    if !(PRIMERA_LPI..PRIMERA_LPI + crate::interrupciones::LPIS).contains(&lpi) {
        return;
    }
    memoria::escribir_fisica(its.config + (lpi - PRIMERA_LPI) as u64, &[CONFIG_LPI | si as u8]);
    let _ = its.orden([0x0D, 0, 0, 0]) && its.sync();
}

/// Una MSI por el ITS para el aparato con ese RID: (dirección donde escribir, dato, LPI que llega). Le da su ITT
/// (MAPD, con un bit de número de suceso) y lleva su suceso 0 a una LPI nueva de la colección 0 (MAPTI).
pub fn msi(rid: u32) -> Option<(u64, u32, u32)> {
    let mut g = ITS.tomar();
    let its = g.as_mut()?;
    if its.aparatos >= APARATOS_ITT || its.siguiente >= PRIMERA_LPI + crate::interrupciones::LPIS {
        return None;
    }
    let itt = its.itt + its.aparatos * TAM_ITT;
    let lpi = its.siguiente;
    let aparato = (rid as u64) << 32;
    if !its.orden([0x08 | aparato, 0, (1 << 63) | itt, 0]) || !its.orden([0x0A | aparato, (lpi as u64) << 32, 0, 0]) || !its.sync() {
        return None;
    }
    its.aparatos += 1;
    its.siguiente += 1;
    Some((its.fisica + GITS_TRANSLATER, 0, lpi))
}
