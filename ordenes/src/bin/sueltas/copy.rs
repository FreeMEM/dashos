//! `Copy` como un programa suelto (`SYS:C/Copy`). La orden está en `ordenes::base::copy`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Copy");
    ordenes::base::copy::principal(&mut o)
}
