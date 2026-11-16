//! `MakeDir` como un programa suelto (`SYS:C/MakeDir`). La orden está en `ordenes::base::makedir`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("MakeDir");
    ordenes::base::makedir::principal(&mut o)
}
