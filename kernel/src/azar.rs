//! Números al azar (C29; R10 de `docs/REVISION_LOOP.md`, S5 de `docs/REVISION_ARQUITECTURA.md`).
//!
//! Un generador ChaCha20 (RFC 8439, sección 2.3) con la llave que se cambia tras cada uso ("fast key erasure"): quien
//! vea el estado después no puede saber lo que salió antes. La llave sale de la semilla que el firmware deja en el
//! árbol de dispositivos (`/chosen/rng-seed` y `/chosen/kaslr-seed`: la ponen el firmware de la Pi y QEMU); en x86, de la CPU (RDSEED o RDRAND). Sin
//! semilla, se mezcla el contador del sistema, que se puede adivinar: vale para colocar programas, no para llaves. La
//! llamada 34 dice cuál de los dos casos es. Falta el generador de la Pi 4 (RNG200), que sumará su parte.

use crate::cerrojo::Cerrojo;

struct Estado {
    llave: [u32; 8],
    contador: u64,
    /// Bytes de semilla de verdad que ha recibido.
    sembrado: usize,
}

static ESTADO: Cerrojo<Estado> = Cerrojo::new(Estado {
    llave: [0x853c_49e6, 0x748f_ea9b, 0x9e37_79b9, 0x7f4a_7c15, 0xbf58_476d, 0x1ce4_e5b9, 0x94d0_49bb, 0x1331_11eb],
    contador: 0,
    sembrado: 0,
});

/// Lo menos de semilla para dar el azar por bueno: 128 bits.
const MINIMO: usize = 16;

fn cuarto(e: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    e[a] = e[a].wrapping_add(e[b]);
    e[d] = (e[d] ^ e[a]).rotate_left(16);
    e[c] = e[c].wrapping_add(e[d]);
    e[b] = (e[b] ^ e[c]).rotate_left(12);
    e[a] = e[a].wrapping_add(e[b]);
    e[d] = (e[d] ^ e[a]).rotate_left(8);
    e[c] = e[c].wrapping_add(e[d]);
    e[b] = (e[b] ^ e[c]).rotate_left(7);
}

/// Un bloque de ChaCha20 (RFC 8439, 2.3): las palabras 12 y 13 son el contador de 64 bits, la 14 el nonce y la 15, 0.
fn bloque(llave: &[u32; 8], contador: u64, nonce: u32) -> [u32; 16] {
    let mut inicial = [0u32; 16];
    inicial[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    inicial[4..12].copy_from_slice(llave);
    inicial[12] = contador as u32;
    inicial[13] = (contador >> 32) as u32;
    inicial[14] = nonce;
    let mut e = inicial;
    for _ in 0..10 {
        cuarto(&mut e, 0, 4, 8, 12);
        cuarto(&mut e, 1, 5, 9, 13);
        cuarto(&mut e, 2, 6, 10, 14);
        cuarto(&mut e, 3, 7, 11, 15);
        cuarto(&mut e, 0, 5, 10, 15);
        cuarto(&mut e, 1, 6, 11, 12);
        cuarto(&mut e, 2, 7, 8, 13);
        cuarto(&mut e, 3, 4, 9, 14);
    }
    for (x, y) in e.iter_mut().zip(inicial.iter()) {
        *x = x.wrapping_add(*y);
    }
    e
}

/// El vector de la RFC 8439, 2.3.2 (llave 00..1f, nonce 00000009 0000004a 00000000, contador 1), para las pruebas
/// del arranque.
pub fn bloque_como_la_rfc() -> bool {
    let llave = core::array::from_fn(|i| u32::from_le_bytes(core::array::from_fn(|k| (4 * i + k) as u8)));
    let b = bloque(&llave, 0x0900_0000_0000_0001, 0x4a00_0000);
    b[0] == 0xe4e7_f110 && b[1] == 0x1559_3bd1 && b[2] == 0x1fdd_0f50 && b[3] == 0xc471_20a3 && b[15] == 0x4e3c_50a2
}

impl Estado {
    /// Mezcla `datos` en la llave: se suman a ella y la llave nueva sale de un bloque con la mezcla.
    fn mezclar(&mut self, datos: &[u8]) {
        for (i, trozo) in datos.chunks(32).enumerate() {
            for (j, b) in trozo.iter().enumerate() {
                self.llave[j / 4] ^= (*b as u32) << (8 * (j % 4));
            }
            let b = bloque(&self.llave, self.contador, 0x6d65_7a63 ^ i as u32);
            self.llave.copy_from_slice(&b[..8]);
            self.contador = self.contador.wrapping_add(1);
        }
    }

    /// Llena `salida` y cambia la llave: lo dado no se puede volver a calcular.
    fn llenar(&mut self, salida: &mut [u8]) {
        // El contador del sistema, también: sin semilla, al menos cambia entre arranques.
        let ahora = crate::reloj::ahora();
        self.llave[7] ^= ahora as u32;
        self.llave[6] ^= (ahora >> 32) as u32;
        for trozo in salida.chunks_mut(64) {
            let b = bloque(&self.llave, self.contador, 1);
            self.contador = self.contador.wrapping_add(1);
            for (i, s) in trozo.iter_mut().enumerate() {
                *s = b[i / 4].to_le_bytes()[i % 4];
            }
        }
        let nueva = bloque(&self.llave, self.contador, 2);
        self.contador = self.contador.wrapping_add(1);
        self.llave.copy_from_slice(&nueva[..8]);
    }
}

/// Siembra con lo que el firmware dejó en `/chosen` y lo borra del árbol original, que está al alcance del DMA y se
/// copia después al montón (`base::copiar_arbol`). Devuelve los bytes de semilla que encontró.
#[cfg(target_arch = "aarch64")]
pub fn sembrar(d: &crate::dtb::Dtb) -> usize {
    let Some(chosen) = d.por_nombre("chosen") else { return 0 };
    let mut total = 0;
    let mut e = ESTADO.tomar();
    for nombre in ["rng-seed", "kaslr-seed"] {
        if let Some((off, largo)) = d.propiedad(&chosen, nombre) {
            let largo = largo.min(256);
            let mut semilla = [0u8; 256];
            for (i, s) in semilla[..largo].iter_mut().enumerate() {
                *s = d.u8(off + i);
            }
            // Una semilla de ceros (la que queda tras borrarla) no cuenta.
            if semilla[..largo].iter().any(|&b| b != 0) {
                e.mezclar(&semilla[..largo]);
                total += largo;
            }
            semilla.fill(0);
            core::hint::black_box(&semilla);
            crate::base::borrar_del_arbol(off, largo);
        }
    }
    e.sembrado += total;
    total
}

/// x86 (X7, paso 4): sin árbol de dispositivos, la semilla sale de la CPU (RDSEED o, sin ella, RDRAND; 256 bytes).
/// Las dos cuentan como semilla de verdad, como hace Linux por omisión (`random.trust_cpu`). Devuelve la fuente y los
/// bytes que dio.
#[cfg(target_arch = "x86_64")]
pub fn sembrar() -> Option<(crate::base::cpu::FuenteAzar, usize)> {
    let mut palabras = [0u64; 32];
    let (fuente, n) = crate::base::cpu::semilla(&mut palabras)?;
    let mut bytes = [0u8; 256];
    for (i, p) in palabras[..n].iter().enumerate() {
        bytes[8 * i..8 * i + 8].copy_from_slice(&p.to_le_bytes());
    }
    let mut e = ESTADO.tomar();
    e.mezclar(&bytes[..8 * n]);
    e.sembrado += 8 * n;
    bytes.fill(0);
    palabras.fill(0);
    core::hint::black_box((&bytes, &palabras));
    Some((fuente, 8 * n))
}

/// Si el azar viene de una semilla de verdad (al menos 128 bits).
pub fn de_calidad() -> bool {
    ESTADO.tomar().sembrado >= MINIMO
}

/// Llena `salida` con bytes al azar.
pub fn llenar(salida: &mut [u8]) {
    ESTADO.tomar().llenar(salida);
}

pub fn u64() -> u64 {
    let mut b = [0u8; 8];
    llenar(&mut b);
    u64::from_le_bytes(b)
}
