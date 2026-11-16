//! Un cliente glotón de la red (S13), solo en `virt`. Llave 1: el canal "red".
//!
//! Abre conexiones con el eco del PC sin parar: el servicio le da `POR_PROGRAMA` y después `LIMITE`, por muchas veces
//! que lo pida. Las tiene abiertas mientras `red-prueba` hace su eco (los demás siguen), las cierra y comprueba que
//! puede abrir otra.
//!
//! S13, segunda parte. Tampoco tiene el permiso de escuchar ni el del reloj, y lo intenta: escuchar por el canal "red",
//! mandar `REENVIADO` (lo que el hilo de escuchar del servicio le pasa al bucle principal) como si fuera el servicio, y
//! poner la hora con cada llave que pueda tener. Y pregunta 2000 nombres de golpe a un servidor que no contesta: el
//! servicio le deja 8 en curso y a las demás dice `LIMITE`.

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::red as p;
use dashos::red_cliente::{estado, Conexion, Oido, Udp};

dashos::programa!(principal);

const SERVICIO: u32 = 1;
const VA: usize = 0x50_0000_0000;
const HUECO: usize = 2 << 20;
const ECO: [u8; 4] = [10, 0, 2, 100];

fn principal() -> u64 {
    let t0 = dashos::ahora_us();
    while !matches!(estado(SERVICIO, 2_000_000), Ok(e) if e[p::E_CONFIGURADA] == 1) {
        if dashos::ahora_us() - t0 > 60_000_000 {
            println!("  [red-gloton] la red no tiene dirección tras 60 s");
            return 1;
        }
        dashos::dormir_us(200_000);
    }
    let mut abiertas: [Option<Conexion>; p::POR_PROGRAMA] = Default::default();
    let mut n = 0;
    // Sin fin: hasta que el servicio diga que no 50 veces seguidas.
    let (mut noes, mut ultimo) = (0, 0);
    let mut intentos = 0;
    while noes < 50 && intentos < 200 {
        intentos += 1;
        let va = VA + n.min(p::POR_PROGRAMA) * HUECO;
        match Conexion::conectar(SERVICIO, ECO, 7, va, 4096, 15_000_000) {
            Ok(c) if n < abiertas.len() => {
                abiertas[n] = Some(c);
                n += 1;
            }
            Ok(c) => {
                println!("  [red-gloton] el servicio da una conexión de más (la {})", n + 1);
                c.cerrar();
                return 2;
            }
            Err(e) => {
                noes += 1;
                ultimo = e;
            }
        }
    }
    let bien = n == p::POR_PROGRAMA && ultimo == p::LIMITE;
    println!("  [red-gloton] tope en {} conexiones; {} intentos más dan {}: {}", n, noes, ultimo, if bien { "bien" } else { "MAL" });
    // Mientras `red-prueba` hace lo suyo (se ve en el total del servicio), se tienen. Como mucho 3 minutos.
    let t1 = dashos::ahora_us();
    let mut visto = false;
    while dashos::ahora_us() - t1 < 180_000_000 {
        let total = estado(SERVICIO, 2_000_000).map(|e| e[p::E_CONEXIONES] as usize).unwrap_or(n);
        if total > n {
            visto = true;
        } else if visto {
            break;
        }
        dashos::dormir_us(200_000);
    }
    for c in abiertas.iter_mut() {
        if let Some(c) = c.take() {
            c.cerrar();
        }
    }
    // Lo cerrado se suelta en el servicio poco después: otra vez puede abrir.
    let t2 = dashos::ahora_us();
    loop {
        match Conexion::conectar(SERVICIO, ECO, 7, VA, 4096, 15_000_000) {
            Ok(c) => {
                c.cerrar();
                println!("  [red-gloton] cerradas las {}; otra vez abre: bien", n);
                break;
            }
            Err(p::LIMITE) if dashos::ahora_us() - t2 < 10_000_000 => dashos::dormir_us(100_000),
            Err(e) => {
                println!("  [red-gloton] cerradas las {}; abrir otra da {} (MAL)", n, e);
                return 3;
            }
        }
    }
    sin_permiso();
    muchas_preguntas();
    0
}

/// Sin los permisos de escuchar y del reloj: no escucha ni pone la hora.
fn sin_permiso() {
    let antes = estado(SERVICIO, 2_000_000).map(|e| e[p::E_RECHAZOS]).unwrap_or(0);
    let tcp = Oido::escuchar(SERVICIO, 8080, VA, 4096, 5_000_000).err();
    let udp = Udp::abrir(SERVICIO, 5353, VA, 4096, 5_000_000).err();
    // Hacerse pasar por el hilo de escuchar del servicio: `REENVIADO` con su propia marca delante y un `ESCUCHAR`.
    let (lee, escribe) = dashos::canal_crear();
    let copia = dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let mut m = [0u8; 24];
    m[..4].copy_from_slice(&p::REENVIADO.to_le_bytes());
    for (i, v) in [p::ESCUCHAR, 8080, 4096].into_iter().enumerate() {
        m[12 + 4 * i..16 + 4 * i].copy_from_slice(&v.to_le_bytes());
    }
    for _ in 0..2000 {
        if dashos::canal_escribir(SERVICIO, &m, &[copia as u32]) != -8 {
            break;
        }
        dashos::dormir_us(1_000);
    }
    let mut r = [0u8; 16];
    let (colado, _, _) = dashos::canal_leer_plazo(lee, &mut r, &mut [], 500_000);
    dashos::cerrar(lee);
    dashos::cerrar(escribe);
    let rechazos = estado(SERVICIO, 2_000_000).map(|e| e[p::E_RECHAZOS]).unwrap_or(0) - antes;
    let bien = tcp == Some(p::SIN_PERMISO) && udp == Some(p::SIN_PERMISO) && colado < 0 && rechazos >= 3;
    println!(
        "  [red-gloton] escuchar sin el permiso: {} (TCP {:?}, UDP {:?}; hacerse pasar por el hilo de escuchar: {}; {} rechazos)",
        if bien { "bien" } else { "MAL" }, tcp, udp, if colado < 0 { "nada" } else { "CONTESTA" }, rechazos
    );
    // La hora: sin el permiso del reloj, ninguna llave suya sirve para ponerla.
    let antes = dashos::hora_ns();
    let puesta = (0..64u32).filter(|&l| dashos::hora_poner(l, 1_900_000_000_000_000_000) == 0).count();
    let igual = dashos::hora_ns().abs_diff(antes) < 2_000_000_000;
    println!("  [red-gloton] poner la hora sin el permiso del reloj, con 64 llaves: {} ({} la ponen; la hora no cambia: {})", if puesta == 0 && igual { "bien" } else { "MAL" }, puesta, igual);
}

/// 2000 preguntas de nombres de golpe, a un servidor que no contesta (el vecino del PC calla con los nombres "mudo"):
/// el servicio deja `RESOLVER_POR_PROGRAMA` en curso y a las demás contesta `LIMITE` al momento.
fn muchas_preguntas() {
    const TOTAL: u32 = 2000;
    let (lee, escribe) = dashos::canal_crear();
    use core::cell::Cell;
    let (limite, sin_respuesta, otras) = (Cell::new(0u32), Cell::new(0u32), Cell::new(0u32));
    let mut perdidas = 0u32;
    let leer = |plazo: u64| -> bool {
        let mut r = [0u8; 64];
        let (n, _, _) = dashos::canal_leer_plazo(lee, &mut r, &mut [], plazo);
        if n < 8 {
            return false;
        }
        match i32::from_le_bytes(r[4..8].try_into().unwrap()) {
            p::LIMITE => limite.set(limite.get() + 1),
            p::SIN_RESPUESTA => sin_respuesta.set(sin_respuesta.get() + 1),
            _ => otras.set(otras.get() + 1),
        }
        true
    };
    let t0 = dashos::ahora_us();
    for i in 0..TOTAL {
        // "mudo-NNNN.prueba", al vecino (10.0.2.53:10053).
        let mut m = [0u8; 16 + 16];
        for (k, v) in [p::RESOLVER, p::IPV4 | 10053 << 16, u32::from_le_bytes([10, 0, 2, 53]), 16].into_iter().enumerate() {
            m[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
        m[16..32].copy_from_slice(b"mudo-0000.prueba");
        for k in 0..4 {
            m[24 - k] = b'0' + (i / 10u32.pow(k as u32) % 10) as u8;
        }
        let copia = dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
        let mut mandada = false;
        for _ in 0..5000 {
            if copia <= 0 {
                break;
            }
            match dashos::canal_escribir(SERVICIO, &m, &[copia as u32]) {
                0 => {
                    mandada = true;
                    break;
                }
                // La cola del servicio, llena: mientras, se leen las respuestas (para que no se llene la propia).
                -8 => {
                    if !leer(1_000) {
                        dashos::dormir_us(500);
                    }
                }
                _ => break,
            }
        }
        if !mandada {
            perdidas += 1;
            if copia > 0 {
                dashos::cerrar(copia as u32);
            }
        }
        while leer(1) {}
    }
    let mandadas_ms = (dashos::ahora_us() - t0) / 1000;
    // Las que quedaron en curso se dan por perdidas a los 7 s.
    while limite.get() + sin_respuesta.get() + otras.get() + perdidas < TOTAL && dashos::ahora_us() - t0 < 60_000_000 {
        leer(1_000_000);
    }
    dashos::cerrar(lee);
    dashos::cerrar(escribe);
    // Después, el servicio sigue: otra conexión.
    let sigue = match Conexion::conectar(SERVICIO, ECO, 7, VA, 4096, 15_000_000) {
        Ok(c) => {
            c.cerrar();
            true
        }
        Err(_) => false,
    };
    let (limite, sin_respuesta, otras) = (limite.get(), sin_respuesta.get(), otras.get());
    let bien = sin_respuesta == p::RESOLVER_POR_PROGRAMA as u32 && limite >= TOTAL - 100 && otras == 0 && sigue;
    println!(
        "  [red-gloton] {} preguntas de nombres de golpe: {} ({} en curso, que acaban sin respuesta; {} con LIMITE; {} que no contestó; mandadas en {} ms; después el servicio sigue: {})",
        TOTAL, if bien { "bien" } else { "MAL" }, sin_respuesta, limite, TOTAL - limite - sin_respuesta - otras, mandadas_ms, sigue
    );
}
