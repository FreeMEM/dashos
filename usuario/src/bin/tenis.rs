//! C32: dos copias de este programa, en núcleos distintos, juegan con los dos extremos del mismo canal (llave 1).
//! Las dos mandan y luego leen, así que a menudo están leyendo a la vez en los dos lados: el caso que colgaba dos
//! núcleos con un cerrojo por lado (auditoría, hallazgo 7). Sale con 32 si acaba todas las vueltas.

#![no_std]
#![no_main]

dashos::programa!(principal);

const VUELTAS: u32 = 20_000;

fn principal() -> u64 {
    let mut buf = [0u8; 4];
    let mut llaves = [0u32; 1];
    for i in 0..VUELTAS {
        if dashos::canal_escribir(1, &i.to_le_bytes(), &[]) != 0 {
            return 1;
        }
        let (n, _) = dashos::canal_leer(1, &mut buf, &mut llaves);
        if n != 4 || u32::from_le_bytes(buf) != i {
            return 2;
        }
    }
    32
}
