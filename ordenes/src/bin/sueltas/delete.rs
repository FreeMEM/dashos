//! `Delete` como un programa suelto (`SYS:C/Delete`). La orden está en `ordenes::base::delete`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Delete");
    ordenes::base::delete::principal(&mut o)
}
