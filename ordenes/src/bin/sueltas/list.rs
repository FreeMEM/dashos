//! `List` como un programa suelto (`SYS:C/List`). La orden está en `ordenes::base::list`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("List");
    ordenes::base::list::principal(&mut o)
}
