//! El buzón de la VideoCore como servicio (C50). Antes iba dentro del kernel (C15).
//!
//! Llaves: la 1, el permiso DMA; la 2, los registros del buzón (la página entera: el buzón está en su desplazamiento
//! 0x880, en todas las Pi); la 3, el canal por el que llegan las peticiones.
//! Cada petición es un mensaje de propiedades del firmware tal cual (palabras de 32 bits en little-endian: tamaño,
//! código 0, etiquetas y la etiqueta 0 de fin), de hasta 256 bytes. La respuesta vuelve por el mismo canal, del mismo
//! tamaño; si el servicio no la acepta, vuelve con el código 0x80000001 y sin tocar el firmware.
//!
//! Solo deja pasar las etiquetas de la lista `PERMITIDAS`: el firmware puede apagar aparatos o cambiar relojes, y
//! eso no lo decide quien tenga el canal. Formato en `docs/referencia/servicios.md`.

#![no_std]
#![no_main]

dashos::programa!(principal);

const VA_REGISTROS: usize = 0x0090_0000;
const VA_MENSAJE: usize = 0x30_0000_0000;

const LEER: usize = 0x00;
const ESTADO: usize = 0x18;
const ESCRIBIR: usize = 0x20;
const LLENO: u32 = 1 << 31;
const VACIO: u32 = 1 << 30;
const CANAL_PROPIEDADES: u32 = 8;

const RECHAZADO: u32 = 0x8000_0001;

/// Las etiquetas que se dejan pasar: leer la memoria de la VideoCore y los relojes, y lo de la pantalla.
const PERMITIDAS: &[u32] = &[
    0x0001_0006, // memoria de la VideoCore: base y tamaño
    0x0003_0002, // frecuencia de un reloj
    0x0003_0004, // frecuencia máxima de un reloj
    0x0004_0001, // reservar la pantalla
    0x0004_0008, // paso de línea
    0x0004_8003, // tamaño físico
    0x0004_8004, // tamaño virtual
    0x0004_8005, // bits por píxel
    0x0004_8006, // orden de los colores
    0x0004_8009, // desplazamiento virtual
];

/// Dónde empieza el buzón 0 dentro de su página (0x7E00B880 en el bus de la VideoCore).
const DESPLAZAMIENTO: usize = 0x880;

fn reg(off: usize) -> *mut u32 {
    (VA_REGISTROS + DESPLAZAMIENTO + off) as *mut u32
}

fn leer(off: usize) -> u32 {
    u32::from_le(unsafe { core::ptr::read_volatile(reg(off)) })
}

/// ¿Es un mensaje bien formado y con solo etiquetas permitidas? `p` son las palabras, ya en el orden de la CPU.
fn valido(p: &[u32]) -> bool {
    if p.len() < 3 || p[0] as usize != p.len() * 4 || p[1] != 0 {
        return false;
    }
    let mut i = 2;
    while i < p.len() {
        let etiqueta = p[i];
        if etiqueta == 0 {
            return true; // lo que haya detrás es relleno
        }
        if i + 3 > p.len() || !PERMITIDAS.contains(&etiqueta) {
            return false;
        }
        let palabras = (p[i + 1] as usize).div_ceil(4);
        i += 3 + palabras;
    }
    false // sin etiqueta de fin
}

/// Pasa el mensaje al firmware y espera la respuesta, con un plazo. Falso si el buzón no contesta.
fn mandar(bus: u64) -> bool {
    let mut n = 0;
    while leer(ESTADO) & LLENO != 0 {
        n += 1;
        if n > 1_000_000 {
            return false;
        }
    }
    unsafe { core::ptr::write_volatile(reg(ESCRIBIR), (((bus as u32) & !0xf) | CANAL_PROPIEDADES).to_le()) };
    for _ in 0..10_000_000 {
        if leer(ESTADO) & VACIO == 0 && leer(LEER) & 0xf == CANAL_PROPIEDADES {
            return true;
        }
    }
    false
}

fn principal() -> u64 {
    if dashos::registros_proyectar(2, VA_REGISTROS) != 0 {
        dashos::println!("  [buzón] sin registros");
        return 1;
    }
    // El mensaje va en memoria DMA del primer GB, sin caché: la VideoCore lo lee y lo escribe sin limpiar cachés.
    let (m, bus) = dashos::memoria_dma(1, 4096);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_MENSAJE, true) != 0 {
        dashos::println!("  [buzón] sin memoria DMA");
        return 2;
    }
    let mensaje = VA_MENSAJE as *mut u32;
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 1];
    loop {
        let (n, _) = dashos::canal_leer(3, &mut buf, &mut ll);
        if n < 0 {
            return 0; // el cliente cerró el canal
        }
        let n = n as usize;
        let mut p = [0u32; 64];
        let palabras = n / 4;
        for (i, w) in p.iter_mut().enumerate().take(palabras) {
            *w = u32::from_le_bytes(buf[4 * i..4 * i + 4].try_into().unwrap());
        }
        let p = &mut p[..palabras];
        if n % 4 != 0 || !valido(p) {
            if p.len() >= 2 {
                p[1] = RECHAZADO;
            }
        } else {
            for (i, &w) in p.iter().enumerate() {
                unsafe { mensaje.add(i).write_volatile(w.to_le()) };
            }
            if mandar(bus) {
                for (i, w) in p.iter_mut().enumerate() {
                    *w = u32::from_le(unsafe { mensaje.add(i).read_volatile() });
                }
            } else {
                p[1] = RECHAZADO;
            }
        }
        for (i, &w) in p.iter().enumerate() {
            buf[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
        }
        dashos::canal_escribir(3, &buf[..n], &[]);
    }
}
