//! C39: el hijo que lanza `padre`. Lee una orden en su llave 1 y la cumple:
//! 1, N: sale con el código N;
//! 2: gira para siempre en EL0 (para matarlo en otro núcleo);
//! 3: falla a propósito (lee la dirección 8).
//! Si no le llega nada, se queda bloqueado leyendo (para matarlo bloqueado).

#![no_std]
#![no_main]

dashos::programa!(principal);

fn principal() -> u64 {
    let mut buf = [0u8; 2];
    let mut llaves = [0u32; 1];
    let (n, _) = dashos::canal_leer(1, &mut buf, &mut llaves);
    if n < 1 {
        return 200;
    }
    match buf[0] {
        1 => buf[1] as u64,
        2 => loop {
            core::hint::spin_loop();
        },
        3 => unsafe { core::ptr::read_volatile(8 as *const u64) },
        _ => 201,
    }
}
