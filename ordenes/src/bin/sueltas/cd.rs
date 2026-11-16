//! `Cd` como un programa suelto (`SYS:C/Cd`). La orden está en `ordenes::base::cd`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Cd");
    ordenes::base::cd::principal(&mut o)
}
