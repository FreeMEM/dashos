//! `Administrar` como un programa suelto (`SYS:C/Administrar`). La orden está en `ordenes::base::administrar`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Administrar");
    ordenes::base::administrar::principal(&mut o)
}
