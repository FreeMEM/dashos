//! `Ask` como un programa suelto (`SYS:C/Ask`). La orden está en `ordenes::base::ask`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Ask");
    ordenes::base::ask::principal(&mut o)
}
