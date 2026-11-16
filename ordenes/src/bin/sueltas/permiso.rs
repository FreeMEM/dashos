//! `Permiso` como un programa suelto (`SYS:C/Permiso`). La orden está en `ordenes::base::permiso`.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    let mut o = ordenes::Orden::arrancar("Permiso");
    ordenes::base::permiso::principal(&mut o)
}
