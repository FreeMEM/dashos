//! `Date` como un programa suelto (`SYS:C/Date`). La orden está en `ordenes::base::date`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Date");
    ordenes::base::date::principal(&mut o)
}
