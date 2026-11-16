//! `Type` como un programa suelto (`SYS:C/Type`). La orden está en `ordenes::base::type_`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Type");
    ordenes::base::type_::principal(&mut o)
}
