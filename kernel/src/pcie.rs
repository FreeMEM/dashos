//! La ventana de entrada del PCIe de la Pi 4 (C51, S16 D): que el VL805 (el xHCI del USB 3) solo alcance el primer
//! GB, donde están los búferes de DMA, y no la memoria de arriba, donde viven el kernel y los procesos (C38).
//!
//! Escrito solo con `docs/spec/hw/pcie.md` (sala limpia); las referencias "§" son a sus secciones. Camino rápido
//! (§11.3): el firmware deja el controlador en marcha y aquí solo se cambian las ventanas, muy pronto en el arranque y
//! antes de que nadie arranque el xHCI. Sin respaldo en Linux, que siempre las programa con el puente en reinicio; el
//! camino seguro (§11.2, reiniciar el enlace) queda para cuando haya controlador xHCI y una Pi en la que probarlo.
//!
//! Sin probar: QEMU `raspi4b` no emula el PCIe (leerlo es un fallo de bus: se sondea).
//!
//! La llave de los registros del controlador (0xFD500000) no se da nunca a un proceso: con ellos se podría volver a
//! abrir la ventana (segunda vuelta de la auditoría, B18). El xHCI solo necesita sus ventanas de salida. Con la ventana
//! en 1 GB, el VL805 sigue alcanzando todo el primer GB, también la memoria de la VideoCore.

use crate::dtb::Dtb;
use crate::kprintln;
use crate::mmio::RegLe32;

// Registros (§3), desde la base del controlador.
const MISC_CTRL: usize = 0x4008;
const RC_BAR1_LO: usize = 0x402C;
const RC_BAR1_HI: usize = 0x4030;
const RC_BAR2_LO: usize = 0x4034;
const RC_BAR2_HI: usize = 0x4038;
const RC_BAR3_LO: usize = 0x403C;
const RC_BAR3_HI: usize = 0x4040;
const MISC_REVISION: usize = 0x406C;

/// `SCB0_SIZE`, bits 31:27 de `MISC_CTRL` (§3.1).
const SCB0_DESP: u32 = 27;
const SCB0_MASCARA: u32 = 0x1F << SCB0_DESP;

/// Tamaño de la ventana: 1 GB, codificado como `log2 − 15` = 15, igual en BAR2 y en `SCB0_SIZE` (§5, §11.1).
pub const TAMANO_1GB: u32 = 15;

/// Codifica un tamaño de ventana de entrada (§5): potencia de dos de 4 KB a 64 GB; 0 si no vale.
pub fn codificar_tamano(bytes: u64) -> u32 {
    if bytes == 0 || !bytes.is_power_of_two() {
        return 0;
    }
    let k = bytes.trailing_zeros();
    match k {
        12..=15 => k - 12 + 0x1C,
        16..=36 => k - 15,
        _ => 0,
    }
}

/// Cierra la ventana de entrada al primer GB. Si no hay controlador (QEMU quita su nodo del árbol), no hace nada.
pub fn cerrar_ventana(d: &Dtb) {
    let Some(n) = d.compatible("brcm,bcm2711-pcie") else {
        kprintln!("PCIe: no está en el árbol de dispositivos: ventana de entrada sin tocar");
        return;
    };
    let Some((fisica, _)) = d.reg_cpu(&n, 0) else { return };
    let base = crate::mmu::virt(fisica as usize);
    let r = |off: usize| RegLe32::new(base + off);
    let Some(revision) = r(MISC_REVISION).sondear() else {
        kprintln!("PCIe: el controlador no contesta (QEMU no lo emula): ventana de entrada sin tocar");
        return;
    };
    // Lo que dejó el firmware, para el registro (§11.3, paso 2).
    let antes = (r(RC_BAR1_LO).leer(), r(RC_BAR2_LO).leer(), r(RC_BAR2_HI).leer(), r(RC_BAR3_LO).leer(), r(MISC_CTRL).leer());
    // BAR1 (a los registros del chip) y BAR3 (otra puerta a la memoria), apagadas (§3.2).
    r(RC_BAR1_LO).escribir(0);
    r(RC_BAR1_HI).escribir(0);
    r(RC_BAR3_LO).escribir(0);
    r(RC_BAR3_HI).escribir(0);
    // BAR2: desplazamiento 0 (bus = física) y 1 GB, en una sola escritura de 32 bits; después SCB0, para no pasar
    // nunca por una BAR2 mayor que la vista SCB (§11.3, pasos 4 y 5).
    r(RC_BAR2_HI).escribir(0);
    r(RC_BAR2_LO).escribir(TAMANO_1GB);
    let c = r(MISC_CTRL).leer();
    r(MISC_CTRL).escribir((c & !SCB0_MASCARA) | (TAMANO_1GB << SCB0_DESP));
    let bien = r(RC_BAR1_LO).leer() == 0 && r(RC_BAR3_LO).leer() == 0 && r(RC_BAR2_LO).leer() == TAMANO_1GB
        && r(RC_BAR2_HI).leer() == 0 && (r(MISC_CTRL).leer() & SCB0_MASCARA) >> SCB0_DESP == TAMANO_1GB;
    kprintln!(
        "PCIe {:#x}: ventana de entrada al primer GB {} (antes BAR1 {:#x}, BAR2 {:#x}:{:#x}, BAR3 {:#x}, MISC_CTRL {:#x})",
        revision, if bien { "cerrada" } else { "MAL" }, antes.0, antes.2, antes.1, antes.3, antes.4
    );
}
