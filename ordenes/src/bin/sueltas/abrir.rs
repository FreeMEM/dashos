//! `Abrir` como un programa suelto (`SYS:C/Abrir`). La orden está en `ordenes::base::abrir`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Abrir");
    ordenes::base::abrir::principal(&mut o)
}
