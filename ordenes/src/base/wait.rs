//! `Wait`: espera. `Wait [n] [SEC|MIN]`: n segundos (1 si no se dice), o minutos. Se corta con Ctrl-C.

use crate::Orden;

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::WAIT);
    let n = args.numero("TIME").unwrap_or(1).clamp(0, 86_400) as u64;
    let hasta = dashos::ahora_us() + n * if args.si("MIN") { 60_000_000 } else { 1_000_000 };
    while dashos::ahora_us() < hasta {
        dashos::dormir_us((hasta - dashos::ahora_us()).min(100_000));
        o.pulso();
    }
    o.fin(shell::RC_BIEN)
}
