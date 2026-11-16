//! `Info` como un programa suelto (`SYS:C/Info`). La orden está en `ordenes::base::info`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Info");
    ordenes::base::info::principal(&mut o)
}
