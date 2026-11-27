//! C32: intenta algo que EL0 no puede hacer y tiene que morir en el intento, sin tumbar el kernel. Qué intenta lo
//! dice el primer byte del mensaje que espera en la llave 1:
//! 1. apagar el temporizador físico (`CNTP_CTL_EL0`; hallazgo 16);
//! 2. leer el contador de ciclos (`PMCCNTR_EL0`; hallazgo 16);
//! 3. ejecutar el código del kernel por su dirección alta (hallazgo 12).
//!
//! En x86-64 (X4): 1, `wrmsr` (el temporizador del APIC); 2, `rdpmc`; 3, saltar a la imagen del kernel; 4, `hlt`.
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
        #[cfg(target_arch = "aarch64")]
        match buf[0] {
            1 => asm!("msr cntp_ctl_el0, xzr"),
            2 => {
                let _c: u64;
                asm!("mrs {}, pmccntr_el0", out(reg) _c);
            }
            3 => asm!("blr {}", in(reg) 0xffff_ff80_0008_0000u64, clobber_abi("C")),
            _ => return 98,
        }
        #[cfg(target_arch = "x86_64")]
        match buf[0] {
            // Apagar el temporizador del APIC: escribir su MSR (wrmsr es del anillo 0).
            1 => asm!("wrmsr", in("ecx") 0x832u32, in("eax") 0x1_0000u32, in("edx") 0u32),
            // Leer un contador de rendimiento: rdpmc solo con CR4.PCE, que el kernel deja apagado.
            2 => {
                let (_a, _d): (u32, u32);
                asm!("rdpmc", in("ecx") 0u32, out("eax") _a, out("edx") _d);
            }
            // Ejecutar el código del kernel (la imagen, en los 2 GB de arriba).
            3 => asm!("call {}", in(reg) 0xffff_ffff_8010_0000u64, clobber_abi("C")),
            // Y la instrucción privilegiada de siempre.
            4 => asm!("hlt"),
            _ => return 98,
        }
    }
    100 + buf[0] as u64
}
