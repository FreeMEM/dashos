//! `Wait` como un programa suelto (`SYS:C/Wait`). La orden está en `ordenes::base::wait`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Wait");
    ordenes::base::wait::principal(&mut o)
}
