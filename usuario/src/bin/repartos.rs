//! C32: muchas copias a la vez en núcleos distintos crean y cierran objetos de memoria sin parar. Sin cerrojo en el
//! repartidor de páginas, dos núcleos se quedaban la misma página o perdían la cuenta de las libres (auditoría,
//! hallazgo 1). Sale con 32.

#![no_std]
#![no_main]

dashos::programa!(principal);

fn principal() -> u64 {
    for _ in 0..300 {
        let k = dashos::memoria_crear(64 * 1024);
        if (k as i32) <= 0 {
            return 1;
        }
        if dashos::llamada(6, [k as u64, 0, 0, 0, 0]).0 != 0 {
            return 2;
        }
    }
    32
}
