//! `Filenote` como un programa suelto (`SYS:C/Filenote`). La orden está en `ordenes::base::filenote`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Filenote");
    ordenes::base::filenote::principal(&mut o)
}
