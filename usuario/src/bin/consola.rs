//! La consola (C48): saca el registro del sistema por el puerto serie. Llave 1: el registro; llave 2: los registros
//! del PL011 (en ARM) o los puertos de COM1 (en x86, X6). Desde que pide el registro por primera vez, el kernel deja
//! el puerto serie y todo sale por aquí.

#![no_std]
#![no_main]

dashos::programa!(principal);

#[cfg(target_arch = "aarch64")]
mod puerto {
    const BASE: usize = 0x0090_0000;
    const DR: usize = 0x00;
    const FR: usize = 0x18;
    const FR_TXFF: u32 = 1 << 5;

    fn reg(off: usize) -> *mut u32 {
        (BASE + off) as *mut u32
    }

    pub fn abrir() -> bool {
        dashos::registros_proyectar(2, BASE) == 0
    }

    /// Con un plazo: si el puerto no avanza, el byte se pierde y la consola sigue.
    pub fn byte(b: u8) {
        for _ in 0..200_000 {
            if unsafe { core::ptr::read_volatile(reg(FR)) } & FR_TXFF == 0 {
                unsafe { core::ptr::write_volatile(reg(DR), b as u32) };
                return;
            }
        }
    }
}

/// COM1, la UART 16550 de un PC: el kernel ya la dejó a 115200 baudios. Se escribe en THR (0x3F8) cuando LSR (0x3FD)
/// dice que hay hueco (bit 5).
#[cfg(target_arch = "x86_64")]
mod puerto {
    const THR: u16 = 0x3f8;
    const LSR: u16 = 0x3fd;
    const LSR_THRE: u8 = 1 << 5;

    pub fn abrir() -> bool {
        dashos::puertos_permitir(2) == 0
    }

    pub fn byte(b: u8) {
        for _ in 0..100_000 {
            let lsr: u8;
            unsafe { core::arch::asm!("in al, dx", out("al") lsr, in("dx") LSR, options(nomem, nostack, preserves_flags)) };
            if lsr & LSR_THRE != 0 {
                break;
            }
            core::hint::spin_loop();
        }
        unsafe { core::arch::asm!("out dx, al", in("dx") THR, in("al") b, options(nomem, nostack, preserves_flags)) };
    }
}

fn principal() -> u64 {
    if !puerto::abrir() {
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
                puerto::byte(b'\r');
            }
            puerto::byte(b);
        }
    }
}
