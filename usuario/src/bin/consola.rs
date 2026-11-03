//! La consola (C48): saca el registro del sistema por el puerto serie. Llave 1: el registro; llave 2: los registros
//! del PL011. Desde que pide el registro por primera vez, el kernel deja el puerto serie y todo sale por aquí.

#![no_std]
#![no_main]

dashos::programa!(principal);

const BASE: usize = 0x0090_0000;
const DR: usize = 0x00;
const FR: usize = 0x18;
const FR_TXFF: u32 = 1 << 5;

fn reg(off: usize) -> *mut u32 {
    (BASE + off) as *mut u32
}

fn byte(b: u8) {
    // Con un plazo: si el puerto no avanza, el byte se pierde y la consola sigue.
    for _ in 0..200_000 {
        if unsafe { core::ptr::read_volatile(reg(FR)) } & FR_TXFF == 0 {
            unsafe { core::ptr::write_volatile(reg(DR), b as u32) };
            return;
        }
    }
}

fn principal() -> u64 {
    if dashos::registros_proyectar(2, BASE) != 0 {
        return 1;
    }
    let mut buf = [0u8; 512];
    loop {
        let n = dashos::llamada(23, [1, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0]).0 as i64;
        if n < 0 {
            return 2;
        }
        for &b in &buf[..n as usize] {
            if b == b'\n' {
                byte(b'\r');
            }
            byte(b);
        }
    }
}
