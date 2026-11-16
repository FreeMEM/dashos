//! `Assign` como un programa suelto (`SYS:C/Assign`). La orden está en `ordenes::base::assign`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Assign");
    ordenes::base::assign::principal(&mut o)
}
