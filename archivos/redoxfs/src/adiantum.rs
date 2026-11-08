//! Adiantum (Crowley y Biggers, 2018): cifrado de bloques de cualquier tamaño que no los alarga, rápido en
//! procesadores sin instrucciones de AES, como el de la Pi 4. Lo usa Android para cifrar el almacenamiento
//! en esos móviles.
//!
//! Escrito desde la especificación de https://github.com/google/adiantum (MIT): XChaCha12 como cifrado en
//! flujo, AES-256 para un solo bloque de 16 bytes y NH + Poly1305 como función hash. Los vectores de prueba
//! de `vectores/` son de ese mismo repositorio.
//!
//! Un bloque se cifra con un "tweak": aquí, su dirección en el disco. El mismo contenido en dos direcciones
//! distintas da dos textos cifrados distintos, y cambiar un solo bit del cifrado cambia todo el bloque al
//! descifrarlo.

use aes::{
    Aes256,
    cipher::{BlockDecrypt, BlockEncrypt, KeyInit, KeyIvInit, StreamCipher},
};
use chacha20::XChaCha12;
use poly1305::{Block, Poly1305, universal_hash::UniversalHash};

/// Bytes de la clave del NH: 8576 bits.
const NH_KEY_BYTES: usize = 1072;
/// El NH trabaja por trozos de 1 KB y da 32 bytes por trozo.
const NH_CHUNK: usize = 1024;
/// El tweak más largo que se acepta.
pub const MAX_TWEAK: usize = 64;

pub struct Adiantum {
    stream_key: [u8; 32],
    aes: Aes256,
    /// Clave de Poly1305 para el tweak.
    k_t: [u8; 16],
    /// Clave de Poly1305 para la salida del NH.
    k_l: [u8; 16],
    /// Clave del NH, en palabras de 32 bits.
    k_n: [u32; NH_KEY_BYTES / 4],
}

impl Adiantum {
    /// Prepara el cifrado con una clave de 32 bytes.
    pub fn new(key: &[u8; 32]) -> Self {
        // K_E || K_H || ... = XChaCha12_K(pad_192(1)): el nonce es un bit a uno y el resto a cero.
        let mut derived = [0u8; 32 + 16 + 16 + NH_KEY_BYTES];
        stream(key, &[], &mut derived);
        let aes = Aes256::new_from_slice(&derived[..32]).expect("clave de AES de 32 bytes");
        let mut k_t = [0; 16];
        k_t.copy_from_slice(&derived[32..48]);
        let mut k_l = [0; 16];
        k_l.copy_from_slice(&derived[48..64]);
        let mut k_n = [0u32; NH_KEY_BYTES / 4];
        for (word, bytes) in k_n.iter_mut().zip(derived[64..].chunks_exact(4)) {
            *word = u32::from_le_bytes(bytes.try_into().unwrap());
        }
        // Que la clave derivada no se quede en la pila.
        derived.fill(0);
        Self {
            stream_key: *key,
            aes,
            k_t,
            k_l,
            k_n,
        }
    }

    /// Cifra `data` en su sitio. Hace falta al menos 16 bytes.
    pub fn encrypt(&self, data: &mut [u8], tweak: &[u8]) {
        assert!(data.len() >= 16 && tweak.len() <= MAX_TWEAK);
        let split = data.len() - 16;
        let (left, right) = data.split_at_mut(split);
        let p_m = le128(right).wrapping_add(self.hash(tweak, left));
        let mut block = p_m.to_le_bytes().into();
        self.aes.encrypt_block(&mut block);
        let c_m: [u8; 16] = block.into();
        stream(&self.stream_key, &c_m, left);
        let c_r = le128(&c_m).wrapping_sub(self.hash(tweak, left));
        right.copy_from_slice(&c_r.to_le_bytes());
    }

    /// Descifra `data` en su sitio.
    pub fn decrypt(&self, data: &mut [u8], tweak: &[u8]) {
        assert!(data.len() >= 16 && tweak.len() <= MAX_TWEAK);
        let split = data.len() - 16;
        let (left, right) = data.split_at_mut(split);
        let c_m = le128(right).wrapping_add(self.hash(tweak, left)).to_le_bytes();
        stream(&self.stream_key, &c_m, left);
        let mut block = c_m.into();
        self.aes.decrypt_block(&mut block);
        let p_m: [u8; 16] = block.into();
        let p_r = le128(&p_m).wrapping_sub(self.hash(tweak, left));
        right.copy_from_slice(&p_r.to_le_bytes());
    }

    /// H(T, L) = Poly1305_KT(|L| en bits || T) + Poly1305_KL(NH(L con ceros hasta múltiplo de 16)).
    fn hash(&self, tweak: &[u8], left: &[u8]) -> u128 {
        let mut head = [0u8; 16 + MAX_TWEAK];
        head[..16].copy_from_slice(&((left.len() as u128) * 8).to_le_bytes());
        head[16..16 + tweak.len()].copy_from_slice(tweak);
        let h_t = poly(&self.k_t).compute_unpadded(&head[..16 + tweak.len()]);

        let mut mac = poly(&self.k_l);
        let mut padded = [0u8; NH_CHUNK];
        for chunk in left.chunks(NH_CHUNK) {
            let len = chunk.len().div_ceil(16) * 16;
            padded[..chunk.len()].copy_from_slice(chunk);
            padded[chunk.len()..len].fill(0);
            let out = nh(&self.k_n, &padded[..len]);
            mac.update(&[Block::clone_from_slice(&out[..16]), Block::clone_from_slice(&out[16..])]);
        }
        let h_l = mac.finalize();
        le128(&h_t).wrapping_add(le128(&h_l))
    }
}

impl Drop for Adiantum {
    fn drop(&mut self) {
        self.stream_key.fill(0);
        self.k_t.fill(0);
        self.k_l.fill(0);
        self.k_n.fill(0);
    }
}

/// XChaCha12 con nonce pad_192(n || 1), sumado con XOR a `data`.
fn stream(key: &[u8; 32], n: &[u8], data: &mut [u8]) {
    let mut nonce = [0u8; 24];
    nonce[..n.len()].copy_from_slice(n);
    nonce[n.len()] = 1;
    XChaCha12::new(key.into(), &nonce.into()).apply_keystream(data);
}

/// Poly1305 como hash: la clave r con la mitad s a cero, así que no se suma nada al final.
fn poly(r: &[u8; 16]) -> Poly1305 {
    let mut key = [0u8; 32];
    key[..16].copy_from_slice(r);
    Poly1305::new(&key.into())
}

/// NH de un trozo de hasta 1 KB (múltiplo de 16): cuatro sumas de productos de 32 x 32 bits.
fn nh(key: &[u32], msg: &[u8]) -> [u8; 32] {
    let mut sums = [0u64; 4];
    for (g, group) in msg.chunks_exact(16).enumerate() {
        let m = |i: usize| u32::from_le_bytes(group[4 * i..4 * i + 4].try_into().unwrap());
        let (m0, m1, m2, m3) = (m(0), m(1), m(2), m(3));
        for (i, sum) in sums.iter_mut().enumerate() {
            let k = &key[4 * g + 4 * i..];
            let a = u64::from(m0.wrapping_add(k[0])) * u64::from(m2.wrapping_add(k[2]));
            let b = u64::from(m1.wrapping_add(k[1])) * u64::from(m3.wrapping_add(k[3]));
            *sum = sum.wrapping_add(a).wrapping_add(b);
        }
    }
    let mut out = [0u8; 32];
    for (i, sum) in sums.iter().enumerate() {
        out[8 * i..8 * i + 8].copy_from_slice(&sum.to_le_bytes());
    }
    out
}

fn le128(bytes: &[u8]) -> u128 {
    u128::from_le_bytes(bytes.try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn hex(s: &str) -> Vec<u8> {
        if s == "-" {
            return Vec::new();
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn nh_vectors() {
        for line in include_str!("vectores/nh.txt").lines() {
            let f: Vec<_> = line.split(' ').map(hex).collect();
            let key: Vec<u32> = f[0]
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            assert_eq!(nh(&key, &f[1])[..], f[2][..]);
        }
    }

    #[test]
    fn nhpoly1305_vectors() {
        for line in include_str!("vectores/nhpoly.txt").lines() {
            let f: Vec<_> = line.split(' ').map(hex).collect();
            let k_l: [u8; 16] = f[0][..16].try_into().unwrap();
            let k_n: Vec<u32> = f[0][16..]
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            let mut mac = poly(&k_l);
            let mut msg = f[1].clone();
            msg.resize(msg.len().div_ceil(16) * 16, 0);
            for chunk in msg.chunks(NH_CHUNK) {
                let out = nh(&k_n, chunk);
                mac.update(&[Block::clone_from_slice(&out[..16]), Block::clone_from_slice(&out[16..])]);
            }
            assert_eq!(mac.finalize()[..], f[2][..]);
        }
    }

    #[test]
    fn adiantum_vectors() {
        let mut count = 0;
        for line in include_str!("vectores/adiantum.txt").lines() {
            let f: Vec<_> = line.split(' ').map(hex).collect();
            let cipher = Adiantum::new(&f[0][..].try_into().unwrap());
            let mut data = f[2].clone();
            cipher.encrypt(&mut data, &f[1]);
            assert_eq!(data, f[3], "cifrar, vector {count}");
            cipher.decrypt(&mut data, &f[1]);
            assert_eq!(data, f[2], "descifrar, vector {count}");
            count += 1;
        }
        assert_eq!(count, 33);
    }

    #[test]
    fn one_bit_changes_everything() {
        let cipher = Adiantum::new(&[7; 32]);
        let mut a = [0u8; 4096];
        let mut b = [0u8; 4096];
        b[100] = 1;
        cipher.encrypt(&mut a, b"tweak");
        cipher.encrypt(&mut b, b"tweak");
        let same = a.iter().zip(b.iter()).filter(|(x, y)| x == y).count();
        assert!(same < 64, "{same} bytes iguales");
        let mut c = [0u8; 4096];
        cipher.encrypt(&mut c, b"tweal");
        assert!(a.iter().zip(c.iter()).filter(|(x, y)| x == y).count() < 64);
    }
}
