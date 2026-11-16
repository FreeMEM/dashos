//! `Echo` como un programa suelto (`SYS:C/Echo`). La orden está en `ordenes::base::echo`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Echo");
    ordenes::base::echo::principal(&mut o)
}
