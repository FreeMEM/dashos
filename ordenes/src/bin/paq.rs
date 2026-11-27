//! El servicio `paq` (S8): el único que escribe en `Paquetes:`. Lo lanza `inicio` con dos llaves: 1, el servicio de
//! archivos (su raíz 0 es `Paquetes`, para leer y escribir; la 1, el disco para leer); 2, su canal, del que solo
//! `sesiones` tiene una copia: por él le llegan las tandas de acciones de administrador ya confirmadas.

#![no_std]
#![no_main]

dashos::programa!(principal);
dashos::monton!();

fn principal() -> u64 {
    ordenes::paq::servicio(1, 2)
}
