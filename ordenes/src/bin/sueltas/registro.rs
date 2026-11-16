//! `Registro` como un programa suelto (`SYS:C/Registro`). La orden está en `ordenes::base::registro`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Registro");
    ordenes::base::registro::principal(&mut o)
}
