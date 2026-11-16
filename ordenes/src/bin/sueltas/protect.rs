//! `Protect` como un programa suelto (`SYS:C/Protect`). La orden está en `ordenes::base::protect`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Protect");
    ordenes::base::protect::principal(&mut o)
}
