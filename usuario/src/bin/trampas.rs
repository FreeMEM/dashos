//! C32: intenta algo que EL0 no puede hacer y tiene que morir en el intento, sin tumbar el kernel. Qué intenta lo
//! dice el primer byte del mensaje que espera en la llave 1:
//! 1. apagar el temporizador físico (`CNTP_CTL_EL0`; hallazgo 16);
//! 2. leer el contador de ciclos (`PMCCNTR_EL0`; hallazgo 16);
//! 3. ejecutar el código del kernel por su dirección alta (hallazgo 12).
//! Si sigue vivo, sale con 100 + el número: el intento funcionó y eso es un fallo.

#![no_std]
#![no_main]

use core::arch::asm;

dashos::programa!(principal);

fn principal() -> u64 {
    let mut buf = [0u8; 1];
    let mut llaves = [0u32; 1];
    let (n, _) = dashos::canal_leer(1, &mut buf, &mut llaves);
    if n != 1 {
        return 99;
    }
    unsafe {
        match buf[0] {
            1 => asm!("msr cntp_ctl_el0, xzr"),
            2 => {
                let _c: u64;
                asm!("mrs {}, pmccntr_el0", out(reg) _c);
            }
            3 => asm!("blr {}", in(reg) 0xffff_ff80_0008_0000u64, clobber_abi("C")),
            _ => return 98,
        }
    }
    100 + buf[0] as u64
}
