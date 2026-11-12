//! La prueba de punta a punta de la red (S13), solo en `virt`. Llave 1: el canal "red".
//!
//! Espera a que el servicio tenga dirección (DHCP) y prueba, contra el eco del PC (`tools/red_eco.py`, que QEMU lanza
//! por cada conexión a 10.0.2.100:7):
//! - que hacerse pasar por el controlador o tocar conexiones ajenas se rechaza;
//! - con el glotón (`red-gloton`) en su tope de conexiones, el eco de 4 MB al azar vuelve igual; mide el caudal y el
//!   tiempo de ida y vuelta;
//! - un puerto cerrado del PC da `RECHAZADA`;
//! - tras la basura que manda el PC (`tools/red_basura.py`: tramas cortas, largas y rotas), el controlador y el
//!   servicio siguen.

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::red as p;
use dashos::red_cliente::{estado, Conexion};

dashos::programa!(principal);

const SERVICIO: u32 = 1;
const VA: usize = 0x50_0000_0000;
const ECO: [u8; 4] = [10, 0, 2, 100];
const PUERTO_ECO: u16 = 7;

/// xorshift64: bytes al azar, repetibles desde la semilla.
struct Azar {
    s: u64,
    v: u64,
    quedan: u32,
}

impl Azar {
    fn nuevo(semilla: u64) -> Self {
        Azar { s: semilla | 1, v: 0, quedan: 0 }
    }

    fn byte(&mut self) -> u8 {
        if self.quedan == 0 {
            self.s ^= self.s << 13;
            self.s ^= self.s >> 7;
            self.s ^= self.s << 17;
            self.v = self.s;
            self.quedan = 8;
        }
        let b = self.v as u8;
        self.v >>= 8;
        self.quedan -= 1;
        b
    }
}

/// Manda `total` bytes al azar por `c` y comprueba que vuelven igual. Devuelve los µs que tardó, o un texto de fallo.
fn eco(c: &Conexion, total: usize, semilla: u64) -> Result<u64, &'static str> {
    let (mut gen_env, mut gen_ver) = (Azar::nuevo(semilla), Azar::nuevo(semilla));
    let mut pend = [0u8; 16384];
    let (mut ini, mut fin) = (0usize, 0usize);
    let (mut generados, mut recibidos) = (0usize, 0usize);
    let mut rbuf = [0u8; 16384];
    let t0 = dashos::ahora_us();
    let limite = t0 + 180_000_000;
    while recibidos < total {
        let mut progreso = false;
        if ini == fin && generados < total {
            let n = pend.len().min(total - generados);
            for b in pend[..n].iter_mut() {
                *b = gen_env.byte();
            }
            generados += n;
            ini = 0;
            fin = n;
        }
        if ini < fin {
            let n = c.enviar(&pend[ini..fin]);
            ini += n;
            progreso |= n > 0;
        }
        let n = c.recibir(&mut rbuf);
        if n > 0 {
            progreso = true;
            if recibidos + n > total {
                return Err("vuelve más de lo enviado");
            }
            for &b in &rbuf[..n] {
                if b != gen_ver.byte() {
                    return Err("vuelve otra cosa");
                }
            }
            recibidos += n;
        }
        if !progreso {
            if c.fase() != p::ABIERTA {
                return Err("la conexión se cerró a medias");
            }
            if dashos::ahora_us() > limite {
                return Err("no acaba");
            }
            c.esperar(true, ini < fin, 1_000_000);
        }
    }
    Ok(dashos::ahora_us() - t0)
}

fn ip(w: u32) -> [u8; 4] {
    w.to_le_bytes()
}

fn principal() -> u64 {
    // La dirección: el servicio la tiene cuando DHCP acaba.
    let t0 = dashos::ahora_us();
    let e = loop {
        match estado(SERVICIO, 2_000_000) {
            Ok(e) if e[p::E_CONFIGURADA] == 1 => break e,
            _ if dashos::ahora_us() - t0 > 60_000_000 => {
                println!("  [red-prueba] la red no tiene dirección tras 60 s");
                return 1;
            }
            _ => dashos::dormir_us(200_000),
        }
    };
    let a = ip(e[p::E_IP]);
    println!("  [red-prueba] la red tiene dirección {}.{}.{}.{} ({} ms desde que empecé a mirar)", a[0], a[1], a[2], a[3], (dashos::ahora_us() - t0) / 1000);

    // Lo que no le toca: decir que es el controlador, avisar y cerrar conexiones que no son suyas, órdenes raras.
    let antes = e[p::E_RECHAZOS];
    let (lee, escribe) = dashos::canal_crear();
    let mem = dashos::memoria_crear(4096);
    let falsa = dashos::llave_duplicar(mem, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let canal = dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let mut tarjeta = [0u8; 12];
    tarjeta[..4].copy_from_slice(&p::TARJETA.to_le_bytes());
    dashos::canal_escribir(SERVICIO, &tarjeta, &[falsa as u32, canal as u32]);
    for orden in [p::AVISO, p::CERRAR] {
        for id in [0u32, 1, 0x100, 0x201, u32::MAX] {
            let mut m = [0u8; 8];
            m[..4].copy_from_slice(&orden.to_le_bytes());
            m[4..].copy_from_slice(&id.to_le_bytes());
            dashos::canal_escribir(SERVICIO, &m, &[]);
        }
    }
    dashos::canal_escribir(SERVICIO, &999u32.to_le_bytes(), &[]);
    dashos::canal_escribir(SERVICIO, &[], &[]);
    dashos::cerrar(lee);
    dashos::cerrar(escribe);
    dashos::cerrar(mem);
    // Mal formados: anillos de más o que no son potencia de dos, una familia que no hay. Y sin canal para contestar.
    let mal = |familia: u32, tam: u32| -> i32 {
        let (lee, escribe) = dashos::canal_crear();
        let ee = dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
        let pet = [p::CONECTAR, familia, PUERTO_ECO as u32, tam, u32::from_le_bytes(ECO), 0, 0, 0];
        let bytes = unsafe { core::slice::from_raw_parts(pet.as_ptr() as *const u8, 32) };
        // Tras la ráfaga de arriba, la cola del servicio puede estar llena (con el PC cargado): se reintenta un rato,
        // en vez de dar por perdida la petición (salía 1, sin respuesta, en vez de MAL_FORMADO).
        for _ in 0..2000 {
            if dashos::canal_escribir(SERVICIO, bytes, &[ee as u32]) != -8 {
                break;
            }
            dashos::dormir_us(1_000);
        }
        let mut r = [0u8; 16];
        let mut ll = [0u32; 8];
        let (n, _, _) = dashos::canal_leer_plazo(lee, &mut r, &mut ll, 2_000_000);
        dashos::cerrar(lee);
        dashos::cerrar(escribe);
        if n >= 12 { i32::from_le_bytes(r[8..12].try_into().unwrap()) } else { 1 }
    };
    let malos = [mal(p::IPV4, 1 << 20), mal(p::IPV4, 3000), mal(6, 4096)];
    let pet = [p::CONECTAR, p::IPV4, PUERTO_ECO as u32, 4096, u32::from_le_bytes(ECO), 0, 0, 0];
    dashos::canal_escribir(SERVICIO, unsafe { core::slice::from_raw_parts(pet.as_ptr() as *const u8, 32) }, &[]);
    let despues = estado(SERVICIO, 2_000_000).map(|e| e[p::E_RECHAZOS]).unwrap_or(0);
    let bien = despues >= antes + 13 && malos == [p::MAL_FORMADO, p::MAL_FORMADO, p::NO_SOPORTADA];
    println!(
        "  [red-prueba] hacerse pasar por el controlador o tocar lo ajeno: {} rechazos; mal formados {:?}: {}",
        despues - antes, malos, if bien { "bien" } else { "MAL" }
    );

    // Con el glotón en su tope: los demás siguen.
    let t1 = dashos::ahora_us();
    let mut tope = 0;
    while dashos::ahora_us() - t1 < 30_000_000 {
        tope = estado(SERVICIO, 2_000_000).map(|e| e[p::E_CONEXIONES]).unwrap_or(0);
        if tope >= p::POR_PROGRAMA as u32 {
            break;
        }
        dashos::dormir_us(100_000);
    }
    let Ok(c) = Conexion::conectar(SERVICIO, ECO, PUERTO_ECO, VA, p::ANILLO_MAX, 15_000_000) else {
        println!("  [red-prueba] no conecta con el eco del PC");
        return 2;
    };
    // Ida y vuelta: 64 bytes, 50 veces.
    let mut tiempos = [0u64; 50];
    let mut b = [0u8; 64];
    let mut azar = Azar::nuevo(0x1234_5678);
    for t in tiempos.iter_mut() {
        for x in b.iter_mut() {
            *x = azar.byte();
        }
        let ida = dashos::ahora_us();
        let mut puesto = 0;
        while puesto < 64 {
            puesto += c.enviar(&b[puesto..]);
        }
        let mut vuelto = [0u8; 64];
        let mut n = 0;
        while n < 64 && c.fase() == p::ABIERTA {
            n += c.recibir(&mut vuelto[n..]);
            if n < 64 {
                c.esperar(true, false, 1_000_000);
            }
        }
        *t = dashos::ahora_us() - ida;
        if vuelto != b {
            println!("  [red-prueba] ida y vuelta: vuelve otra cosa");
            return 3;
        }
    }
    tiempos.sort_unstable();
    println!("  [red-prueba] ida y vuelta de 64 bytes, 50 veces: mediana {} µs, mínimo {} µs, máximo {} µs", tiempos[25], tiempos[0], tiempos[49]);
    let mut semilla = [0u8; 8];
    dashos::azar(&mut semilla);
    let total = 4 << 20;
    match eco(&c, total, u64::from_le_bytes(semilla)) {
        Ok(us) => {
            // MB/s con dos decimales, en enteros: lo enviado (y lo vuelto es lo mismo).
            let centesimas = total as u64 * 100 / us.max(1);
            println!("  [red-prueba] eco de 4 MB al azar: vuelve igual: bien");
            println!("  [red-prueba] caudal: {}.{:02} MB/s de ida (y lo mismo de vuelta), {} ms", centesimas / 100, centesimas % 100, us / 1000);
        }
        Err(m) => {
            println!("  [red-prueba] eco de 4 MB: {}", m);
            return 4;
        }
    }
    println!("  [red-prueba] con el glotón en su tope ({} conexiones abiertas): {}", tope, if tope >= p::POR_PROGRAMA as u32 { "bien" } else { "MAL, no llegó" });
    c.cerrar();

    // Un puerto cerrado del PC (10.0.2.2 es el PC para QEMU): el otro lado contesta que no.
    match Conexion::conectar(SERVICIO, [10, 0, 2, 2], 1, VA, 4096, 15_000_000) {
        Err(p::RECHAZADA) => println!("  [red-prueba] puerto cerrado: rechazada: bien"),
        Err(e) => println!("  [red-prueba] puerto cerrado: {} (MAL)", e),
        Ok(c) => {
            println!("  [red-prueba] puerto cerrado: se abre (MAL)");
            c.cerrar();
        }
    }

    // La basura del PC: el controlador tira lo corto y lo largo; la pila, lo roto. Después, el eco sigue.
    let t2 = dashos::ahora_us();
    let mut e = estado(SERVICIO, 2_000_000).unwrap_or([0; p::E_PALABRAS]);
    while (e[p::E_CORTAS] == 0 || e[p::E_LARGAS] == 0) && dashos::ahora_us() - t2 < 60_000_000 {
        dashos::dormir_us(500_000);
        e = estado(SERVICIO, 2_000_000).unwrap_or(e);
    }
    let otra = Conexion::conectar(SERVICIO, ECO, PUERTO_ECO, VA, 64 * 1024, 15_000_000);
    let sigue = match &otra {
        Ok(c) => eco(c, 256 * 1024, 77).is_ok(),
        Err(_) => false,
    };
    if let Ok(c) = otra {
        c.cerrar();
    }
    let e = estado(SERVICIO, 2_000_000).unwrap_or(e);
    println!(
        "  [red-prueba] tramas: {} recibidas, {} enviadas; tiradas: {} cortas, {} largas, {} sin sitio",
        e[p::E_RECIBIDAS], e[p::E_ENVIADAS], e[p::E_CORTAS], e[p::E_LARGAS], e[p::E_SIN_SITIO]
    );
    let bien = sigue && e[p::E_CORTAS] > 0 && e[p::E_LARGAS] > 0;
    println!("  [red-prueba] basura del PC tirada; eco después: {}", if bien { "bien" } else { "MAL" });
    0
}
