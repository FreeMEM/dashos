//! X13: la prueba de la ventana compartida con el dominio de gráficos (`dashos::protocolo::compartida`). Hace lo que hará
//! el compositor con el búfer de una ventana: crea su memoria, la pinta, la pone en el dominio por el canal "graficos"
//! y, cuadro a cuadro, la vuelve a pintar y dice que está lista; espera a que el dominio diga que la vio antes de tocarla
//! otra vez. Ninguna copia: el dominio lee la misma memoria.
//!
//! El dibujo son bloques de 16 x 16 que cambian con el cuadro (`dibujo`; la misma cuenta, en
//! `caja-linux/dominios/graficos_dominio.c`, que la comprueba en la memoria y en la pantalla).
//!
//! Dice lo que tardó cada ida y vuelta (de "listo" a "visto": el dominio lo sube y lo cambia a la pantalla, al ritmo de
//! su refresco) y lo que le cuesta pintar. Sale con 0 si el dominio vio todos los cuadros.

#![no_std]
#![no_main]

extern crate alloc;

use dashos::println;
use dashos::protocolo::compartida as c;

dashos::programa!(principal);
dashos::monton!();

const ANCHO: u32 = 640;
const ALTO: u32 = 480;
const CUADROS: u64 = 300;
/// Donde se proyecta aquí el búfer.
const VA: usize = 0x40_0000_0000;

/// El cuadro `k`: bloques de 16 x 16, en XRGB8888.
fn dibujo(x: u32, y: u32, k: u64) -> u32 {
    let (bx, by, kk) = (x / 16, y / 16, k as u32);
    let r = (bx.wrapping_mul(7).wrapping_add(kk.wrapping_mul(3))) & 255;
    let g = (by.wrapping_mul(11).wrapping_add(kk.wrapping_mul(5))) & 255;
    let b = ((bx ^ by).wrapping_mul(13).wrapping_add(kk)) & 255;
    0xff00_0000 | r << 16 | g << 8 | b
}

fn pintar(px: &mut [u32], k: u64) {
    for y in 0..ALTO {
        let fila = &mut px[(y * ANCHO) as usize..((y + 1) * ANCHO) as usize];
        for (x, p) in fila.iter_mut().enumerate() {
            *p = dibujo(x as u32, y, k);
        }
    }
}

fn palabras(p: &[u32]) -> alloc::vec::Vec<u8> {
    p.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Espera un mensaje de `lee` de como mucho `plazo_us`: sus palabras, o `None`.
fn esperar(lee: u32, plazo_us: u64) -> Option<[u32; 4]> {
    let mut b = [0u8; 16];
    let mut ll = [0u32; 1];
    let (n, k, _) = dashos::canal_leer_plazo(lee, &mut b, &mut ll, plazo_us);
    for &l in &ll[..k.min(1)] {
        dashos::cerrar(l);
    }
    if n < 8 {
        return None;
    }
    let w = |i: usize| u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
    Some([w(0), w(1), w(2), w(3)])
}

fn principal() -> u64 {
    let Some(g) = dashos::llaves::buscar("graficos") else {
        println!("  [graficos-cliente] sin el canal \"graficos\"");
        return 2;
    };
    let tam = (ANCHO * ALTO * 4) as usize;
    let mem = dashos::memoria_crear(tam);
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA, true) != 0 {
        println!("  [graficos-cliente] sin memoria para el búfer");
        return 2;
    }
    // SAFETY: recién proyectado aquí, de `tam` bytes, para escribir, y no se quita.
    let px: &mut [u32] = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, tam / 4) };
    pintar(px, 1);
    let (lee, escribe) = dashos::canal_crear();
    let copia = dashos::llave_duplicar(mem, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    if copia <= 0 {
        println!("  [graficos-cliente] no se puede copiar la llave del búfer: {}", copia);
        return 2;
    }
    let pide = palabras(&[c::PONER, ANCHO, ALTO, ANCHO * 4, c::XRGB]);
    if dashos::canal_escribir(g, &pide, &[copia as u32, escribe]) < 0 {
        println!("  [graficos-cliente] el servicio no recibe");
        return 3;
    }
    let ranura = match esperar(lee, 30_000_000) {
        Some([c::PONER, r, ..]) if (r as i32) >= 0 => r,
        otra => {
            println!("  [graficos-cliente] no se pudo poner la ventana: {:?}", otra.map(|w| w[1] as i32));
            return 3;
        }
    };
    println!("  [graficos-cliente] ventana de {}x{} en la ranura {} del dominio", ANCHO, ALTO, ranura);
    let (mut vueltas, mut peor, mut pintado) = (0u64, 0u64, 0u64);
    let mut vistos = 0u64;
    for k in 1..=CUADROS {
        if k > 1 {
            let t = dashos::ahora_us();
            pintar(px, k);
            pintado += dashos::ahora_us() - t;
        }
        let t = dashos::ahora_us();
        if dashos::canal_escribir(g, &palabras(&[c::LISTO, ranura, k as u32, (k >> 32) as u32]), &[]) < 0 {
            println!("  [graficos-cliente] el servicio ya no recibe (cuadro {})", k);
            break;
        }
        // El primero espera a que arranque el dominio y acabe lo suyo (unos 25 s); los demás, un refresco o dos.
        let plazo = if k == 1 { 120_000_000 } else { 5_000_000 };
        let mut visto = false;
        while let Some([c::VISTO, r, bajo, alto]) = esperar(lee, plazo) {
            if r == ranura && (bajo as u64 | (alto as u64) << 32) >= k {
                visto = true;
                break;
            }
        }
        if !visto {
            println!("  [graficos-cliente] el dominio no vio el cuadro {}", k);
            break;
        }
        vistos = k;
        if k > 1 {
            let d = dashos::ahora_us() - t;
            vueltas += d;
            peor = peor.max(d);
        }
    }
    let _ = dashos::canal_escribir(g, &palabras(&[c::ACABAR, ranura]), &[]);
    let n = vistos.saturating_sub(1).max(1);
    println!(
        "  [graficos-cliente] {} de {} cuadros vistos por el dominio; de listo a visto {} µs de media, {} el peor; pintar {} µs por cuadro",
        vistos,
        CUADROS,
        vueltas / n,
        peor,
        pintado / n
    );
    if vistos == CUADROS { 0 } else { 1 }
}
