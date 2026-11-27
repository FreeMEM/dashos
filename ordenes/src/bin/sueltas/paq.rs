//! `Paq`, el gestor de paquetes (S8), como programa suelto (`SYS:C/Paq`). La orden está en `ordenes::paq`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Paq");
    ordenes::paq::principal(&mut o)
}
