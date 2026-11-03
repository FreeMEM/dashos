//! C46: memoria DMA. Llave 1: un permiso DMA del primer GB (lo da el kernel). Sale con 46 si todo va bien, o con el
//! número del paso que falló.

#![no_std]
#![no_main]

use dashos::{memoria_dma, memoria_proyectar, println};

dashos::programa!(principal);

const E_TIPO: i64 = -6;
const E_TAMANO: i64 = -5;

fn principal() -> u64 {
    // 1: 64 KB seguidos; su dirección de bus cae en la ventana de la SD (0xC000_0000 a 0x1_0000_0000).
    let (m, bus) = memoria_dma(1, 64 * 1024);
    if m <= 0 || !(0xC000_0000..0x1_0000_0000 - 64 * 1024).contains(&bus) || bus % 4096 != 0 {
        return 1;
    }
    // 2: se proyecta (sin caché) y se lee lo que se escribe.
    let va = 0x30_0000_0000usize;
    if memoria_proyectar(m as u32, va, true) != 0 {
        return 2;
    }
    unsafe {
        core::ptr::write_volatile((va + 0xfff8) as *mut u64, 0x1234);
        if core::ptr::read_volatile((va + 0xfff8) as *const u64) != 0x1234 || core::ptr::read_volatile(va as *const u64) != 0 {
            return 3;
        }
    }
    // 4: sin permiso, no: con una llave de memoria normal da E_TIPO.
    let normal = dashos::memoria_crear(4096);
    if memoria_dma(normal, 4096).0 != E_TIPO {
        return 4;
    }
    // 5: más de 4 MB de una vez, E_TAMANO.
    if memoria_dma(1, 8 << 20).0 != E_TAMANO {
        return 5;
    }
    // 6: la memoria normal de un proceso nunca sale del primer GB; la DMA, siempre.
    println!("  [dma] 64 KB seguidos con dirección de bus {:#x}, sin permiso no", bus);
    46
}
