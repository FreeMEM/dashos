//! `Usuario` como un programa suelto (`SYS:C/Usuario`). La orden está en `ordenes::base::usuario`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Usuario");
    ordenes::base::usuario::principal(&mut o)
}
