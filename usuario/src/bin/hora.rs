//! La hora por la red (S13): un cliente de SNTP que pone el reloj del sistema. Es el único programa con el permiso del
//! reloj (la llamada 39 con 1): poner la hora es una acción del sistema, y la hace este servicio, no quien se lo pida.
//! De la red solo tiene la llave de conectar, como cualquier programa: no escucha.
//!
//! El servidor lo dice el servicio de red en su estado (`E_HORA`): el de `Prefs/Red/hora` del disco del sistema, ya
//! resuelto. Cuando hay red y servidor, le pregunta (`dashos::sntp`): la petición lleva un número al azar y solo vale
//! la respuesta que lo devuelve, de un servidor con hora, desde la dirección a la que se preguntó (el zócalo, sin el
//! permiso de escuchar, no recibe de nadie más). Pone la hora y vuelve a preguntar cada hora; si nadie contesta,
//! reintenta a los 30 s, y cada vez más despacio (hasta 10 minutos).
//!
//! La hora se pone de golpe (un salto), también hacia atrás: no hay ajuste fino del ritmo del reloj.
//!
//! Llaves: 1 el permiso del reloj, 2 el canal "red".

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::red as p;
use dashos::red_cliente::{estado, Udp};
use dashos::sntp;

dashos::programa!(principal);

const RELOJ: u32 = 1;
const RED: u32 = 2;
const VA: usize = 0x50_0000_0000;
/// Intentos de cada vez y lo que se espera cada respuesta.
const INTENTOS: u32 = 4;
const ESPERA_US: u64 = 2_000_000;
/// Cada cuánto se vuelve a preguntar tras acertar, y tras fallar (se dobla hasta el tope).
const CADA_US: u64 = 3_600_000_000;
const REINTENTO_US: u64 = 30_000_000;
const REINTENTO_MAX_US: u64 = 600_000_000;

/// Duerme `us` (la llamada deja como mucho una hora de una vez).
fn dormir(mut us: u64) {
    while us > 0 {
        let trozo = us.min(60_000_000);
        dashos::dormir_us(trozo);
        us -= trozo;
    }
}

/// Pregunta la hora a `ip` y la pone. Cuántas respuestas que no valían se tiraron, o `Err` si nadie contestó bien.
fn sincronizar(ip: [u8; 4]) -> Result<u32, u32> {
    let Ok(z) = Udp::abrir(RED, 0, VA, p::ANILLO_MIN, 5_000_000) else { return Err(0) };
    let mut tiradas = 0;
    let mut buf = [0u8; 128];
    for _ in 0..INTENTOS {
        let mut numero = [0u8; 8];
        dashos::azar(&mut numero);
        let numero = u64::from_le_bytes(numero);
        let ida = dashos::ahora_ns();
        if !z.enviar4(&sntp::peticion(numero), ip, sntp::PUERTO) {
            break;
        }
        let fin = dashos::ahora_us() + ESPERA_US;
        while dashos::ahora_us() < fin && z.fase() == p::ABIERTA {
            let Some((n, _, _, _)) = z.recibir(&mut buf) else {
                z.esperar(fin.saturating_sub(dashos::ahora_us()).max(1));
                continue;
            };
            let vuelta = dashos::ahora_ns();
            match sntp::respuesta(&buf[..n.min(buf.len())], numero) {
                Ok(h) => {
                    let ns = sntp::al_recibir(&h, vuelta - ida);
                    let antes = dashos::hora_ns();
                    let r = dashos::hora_poner(RELOJ, ns);
                    z.cerrar();
                    if r != 0 {
                        println!("  [hora] el kernel no deja poner la hora: {}", r);
                        return Err(tiradas);
                    }
                    let (ms, como) = if antes == 0 {
                        (0, "no había hora")
                    } else if antes > ns {
                        ((antes - ns) / 1_000_000, "iba adelantado")
                    } else {
                        ((ns - antes) / 1_000_000, "iba atrasado")
                    };
                    println!("  [hora] reloj puesto por SNTP de {}.{}.{}.{}: {} ({} ms); ida y vuelta, {} µs", ip[0], ip[1], ip[2], ip[3], como, ms, (vuelta - ida) / 1000);
                    return Ok(tiradas);
                }
                Err(_) => tiradas += 1,
            }
        }
    }
    z.cerrar();
    Err(tiradas)
}

fn principal() -> u64 {
    // Hostil, contra uno mismo: una hora que no es creíble (1999) no se pone, ni con el permiso; y con una llave que no
    // es el permiso (el canal de la red), tampoco.
    let antes = dashos::hora_ns();
    let vieja = dashos::hora_poner(RELOJ, 946_684_800_000_000_000);
    let otra_llave = dashos::hora_poner(RED, antes.max(1_800_000_000_000_000_000));
    let igual = dashos::hora_ns().abs_diff(antes) < 1_000_000_000;
    println!(
        "  [hora] una hora que no es creíble o una llave que no es el permiso no ponen el reloj: {} (una hora de 1999 da {}; con otra llave, {}; la hora no cambia: {})",
        if vieja < 0 && otra_llave < 0 && igual { "bien" } else { "MAL" }, vieja, otra_llave, igual
    );
    let mut espera = REINTENTO_US;
    let mut dicho = false;
    loop {
        // El servidor, cuando el servicio de red tenga dirección y lo haya resuelto.
        let ip = match estado(RED, 2_000_000) {
            Ok(e) if e[p::E_CONFIGURADA] == 1 && e[p::E_HORA] != 0 => e[p::E_HORA].to_le_bytes(),
            _ => {
                dashos::dormir_us(1_000_000);
                continue;
            }
        };
        match sincronizar(ip) {
            Ok(tiradas) => {
                if tiradas > 0 {
                    println!("  [hora] respuestas que no valían, tiradas: {}", tiradas);
                }
                espera = REINTENTO_US;
                dicho = false;
                dormir(CADA_US);
            }
            Err(tiradas) => {
                if !dicho {
                    println!("  [hora] {}.{}.{}.{} no da la hora ({} respuestas que no valían); se reintenta", ip[0], ip[1], ip[2], ip[3], tiradas);
                    dicho = true;
                }
                dormir(espera);
                espera = (espera * 2).min(REINTENTO_MAX_US);
            }
        }
    }
}
