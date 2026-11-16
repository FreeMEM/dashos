//! `Dir` como un programa suelto (`SYS:C/Dir`). La orden está en `ordenes::base::dir`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Dir");
    ordenes::base::dir::principal(&mut o)
}
