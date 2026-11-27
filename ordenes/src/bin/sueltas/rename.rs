//! `Rename` como un programa suelto (`SYS:C/Rename`). La orden está en `ordenes::base::rename`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Rename");
    ordenes::base::rename::principal(&mut o)
}
