//! `presion` (aviso-cajas, solo en las imágenes de prueba): provoca el aviso de memoria baja del kernel para ver que
//! las cajas sueltan memoria.
//!
//! Llave 1: el lado de leer del canal "presion". Por él le dicen las cajas que tienen algo que soltar ("caja-amiga"
//! cuando su 68k ha crecido y tiene un manejador de memoria puesto, "caja-linux" cuando su invitado está en marcha).
//! Con cada aviso, llena la memoria con objetos de memoria hasta dejar libre un 4 % de lo que había para procesos al
//! arrancar (el kernel avisa por debajo del 5 %), suelta hasta quedar en un 7 % (sigue siendo memoria baja: el kernel no
//! dice que se acabó hasta pasar del 10 %), espera a que las cajas suelten y lo suelta todo. Su grupo tiene un tope de
//! 1 TB (lo pone `inicio`): lo que no pasa es lo que hay de verdad.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const CANAL: u32 = 1;
const GRANDE: usize = 16 << 20;
const PEQUENO: usize = 1 << 20;
/// Lo que espera con la memoria baja, para que las cajas suelten (el globo de Linux, los manejadores de la Amiga).
const ESPERA_US: u64 = 10_000_000;
/// Hasta cuándo escucha a las cajas.
const PLAZO_US: u64 = 900_000_000;

fn libres() -> u64 {
    dashos::medir(15, 0).max(0) as u64
}

/// Llena hasta que lo libre baje de `hasta` bytes; devuelve las llaves.
fn llenar(hasta: u64, llaves: &mut [u32; 1000], n: &mut usize) {
    while *n < llaves.len() {
        let l = libres();
        let trozo = if l > hasta + GRANDE as u64 { GRANDE } else if l > hasta { PEQUENO } else { break };
        let k = dashos::memoria_crear(trozo) as i32;
        if k <= 0 {
            break;
        }
        llaves[*n] = k as u32;
        *n += 1;
    }
}

fn principal() -> u64 {
    let base = dashos::medir(16, 0).max(0) as u64;
    if base == 0 {
        println!("  [presion] el kernel no dice cuánta memoria hay; nada que hacer");
        return 1;
    }
    let mut b = [0u8; 32];
    let hasta = dashos::ahora_us() + PLAZO_US;
    let mut llaves = [0u32; 1000];
    loop {
        let quedan = hasta.saturating_sub(dashos::ahora_us());
        if quedan == 0 {
            return 0;
        }
        let (n, _, marca) = dashos::canal_leer_plazo(CANAL, &mut b, &mut [], quedan);
        if n < 0 {
            return 0;
        }
        // Lo de `inicio` (sin marca: quién es cada cliente del canal) no cuenta; solo lo que dicen las cajas.
        let quien = match &b[..n as usize] {
            b"caja-amiga" => "caja-amiga",
            b"caja-linux" => "caja-linux",
            _ => continue,
        };
        if marca == 0 {
            continue;
        }
        let antes = libres();
        let mut k = 0;
        llenar(base * 4 / 100, &mut llaves, &mut k);
        let lleno = libres();
        // Hasta el 7 %: memoria baja todavía, pero con sitio para los demás mientras las cajas sueltan.
        while k > 0 && libres() < base * 7 / 100 {
            k -= 1;
            dashos::cerrar(llaves[k]);
        }
        println!(
            "  [presion] {} tiene algo que soltar: se llena la memoria de {} MB libres a {} MB (el aviso salta por debajo de {} MB) y se deja en {} MB",
            quien,
            antes >> 20,
            lleno >> 20,
            base / 20 >> 20,
            libres() >> 20
        );
        dashos::dormir_us(ESPERA_US);
        let tras = libres();
        for &l in &llaves[..k] {
            dashos::cerrar(l);
        }
        println!("  [presion] tras {} s, {} MB libres; se suelta todo y quedan {} MB", ESPERA_US / 1_000_000, tras >> 20, libres() >> 20);
    }
}
