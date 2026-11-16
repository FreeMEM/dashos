//! La prueba de punta a punta del dominio de red (S35, paso 2; `docs/bitacoras/dominio-red.md`): la red de DashOS
//! sale por una tarjeta que lleva un Linux en un dominio y que DashOS usa como una virtio-net.
//!
//! Por fases, cada una con el dominio en un modo (lo pide por "dominio-control" al servicio `dominio`, que lo relanza):
//!
//! 1. **Normal**: DHCP (de la red de usuario de QEMU, en el mismo concentrador que la tarjeta), una conexión TCP con el
//!    eco del PC (10.0.2.100:7, por la red de QEMU, sin salir de él) y el eco UDP del vecino de prueba
//!    (`tools/red_vecino.py`, 10.0.2.53:4007). Nunca Internet.
//! 2. **Hostil: oferta, descriptores y fuera**: el dominio miente en su oferta, devuelve usados que no son del
//!    conductor, lanza una tormenta de avisos e intenta leer fuera de su memoria. La red sigue: otro eco TCP.
//! 3. **Hostil: se cuelga**: deja de atender y de latir con un eco TCP en marcha; el vigilante lo relanza y el eco acaba.
//! 4. **Hostil: muere a media transferencia**: acaba su /init (pánico del kernel) con un eco TCP en marcha; se relanza y
//!    el eco acaba.
//!
//! Llaves: la 1, "red" (el permiso de red); la 2, "dominio-control" (lo que avisa el servicio del dominio).

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::red as p;
use dashos::red_cliente::{estado, Conexion, Udp};

dashos::programa!(principal);

const SERVICIO: u32 = 1;
const CONTROL: u32 = 2;
const VA: usize = 0x50_0000_0000;
const VA2: usize = VA + (4 << 20);
const ECO: [u8; 4] = [10, 0, 2, 100];
const VECINO: [u8; 4] = [10, 0, 2, 53];
const P_ECO_VECINO: u16 = 4007;

const C_HOLA: u32 = 1;
const C_RELANZAR: u32 = 2;
const C_LISTO: u32 = 3;
const C_CAIDO: u32 = 4;

/// Bytes al azar, repetibles con la misma semilla.
struct Azar(u64);

impl Azar {
    fn byte(&mut self) -> u8 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 24) as u8
    }
}

/// Manda `total` bytes por `c` y comprueba que vuelven iguales. Microsegundos que tardó, o qué falló.
fn eco(c: &Conexion, total: usize, semilla: u64, plazo_us: u64) -> Result<u64, &'static str> {
    let (mut env, mut ver) = (Azar(semilla), Azar(semilla));
    let mut pend = [0u8; 8192];
    let (mut ini, mut fin) = (0usize, 0usize);
    let (mut generados, mut recibidos) = (0usize, 0usize);
    let mut rbuf = [0u8; 8192];
    let t0 = dashos::ahora_us();
    while recibidos < total {
        let mut progreso = false;
        if ini == fin && generados < total {
            let n = pend.len().min(total - generados);
            for b in pend[..n].iter_mut() {
                *b = env.byte();
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
            if rbuf[..n].iter().any(|&b| b != ver.byte()) {
                return Err("vuelve otra cosa");
            }
            recibidos += n;
        }
        if !progreso {
            if c.fase() != p::ABIERTA {
                return Err("la conexión se cerró a medias");
            }
            if dashos::ahora_us() - t0 > plazo_us {
                return Err("no acaba");
            }
            c.esperar(true, ini < fin, 500_000);
        }
    }
    Ok(dashos::ahora_us() - t0)
}

/// Un eco TCP de `total` bytes con el eco del PC; dice cómo fue.
fn eco_tcp(fase: &str, total: usize, semilla: u64, plazo_us: u64) -> bool {
    match Conexion::conectar(SERVICIO, ECO, 7, VA, 64 * 1024, 20_000_000) {
        Ok(c) => {
            let r = eco(&c, total, semilla, plazo_us);
            c.cerrar();
            match r {
                Ok(us) => {
                    println!("  [dominio-prueba] {}: eco TCP de {} KB con 10.0.2.100:7 por el dominio en {} ms: bien", fase, total / 1024, us / 1000);
                    true
                }
                Err(e) => {
                    println!("  [dominio-prueba] {}: eco TCP de {} KB: {}; MAL", fase, total / 1024, e);
                    false
                }
            }
        }
        Err(e) => {
            println!("  [dominio-prueba] {}: no conecta con 10.0.2.100:7 ({}); MAL", fase, e);
            false
        }
    }
}

/// El eco UDP del vecino de prueba.
fn eco_udp() -> bool {
    let Ok(z) = Udp::abrir(SERVICIO, 0, VA2, 16 * 1024, 5_000_000) else {
        println!("  [dominio-prueba] no abre un zócalo UDP; MAL");
        return false;
    };
    let mut buf = [0u8; 256];
    let mut bien = false;
    for intento in 0..5 {
        let m = b"hola, vecino, desde DashOS por el dominio";
        z.enviar4(m, VECINO, P_ECO_VECINO);
        let fin = dashos::ahora_us() + 2_000_000;
        while dashos::ahora_us() < fin {
            if let Some((n, d, _, puerto)) = z.recibir(&mut buf) {
                if d[..4] == VECINO && puerto == P_ECO_VECINO && &buf[..n.min(buf.len())] == m {
                    bien = true;
                    break;
                }
            }
            z.esperar(200_000);
        }
        if bien {
            println!("  [dominio-prueba] eco UDP con el vecino 10.0.2.53:4007: bien ({} intentos)", intento + 1);
            break;
        }
    }
    if !bien {
        println!("  [dominio-prueba] eco UDP con el vecino: sin respuesta; MAL");
    }
    z.cerrar();
    bien
}

/// Lo que avisa el servicio del dominio: (tipo, generación, motivo), o nada en el plazo.
fn aviso(plazo_us: u64) -> Option<(u32, u32, u32)> {
    let mut b = [0u8; 256];
    let mut ll = [0u32; 4];
    let (n, k, _) = dashos::canal_leer_plazo(CONTROL, &mut b, &mut ll, plazo_us);
    if n < 0 {
        return None;
    }
    ll[..k.min(ll.len())].iter().for_each(|&l| {
        dashos::cerrar(l);
    });
    let w = |i: usize| u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap_or([0; 4]));
    Some((w(0), w(1), w(2)))
}

/// Los motivos de caída que dice el servicio (`Fin::numero` en `dominio.rs`).
const M_REINICIO: u32 = 2;
const M_COLGADO: u32 = 4;
const M_ORDEN: u32 = 7;

/// Espera a que caiga la generación del dominio que está en marcha por `motivo` (cualquiera, si es 0) y a que la
/// siguiente tenga su aparato listo: el número de esta.
fn esperar_vuelta(motivo: u32, plazo_us: u64) -> Option<u32> {
    let fin = dashos::ahora_us() + plazo_us;
    let mut cayo = motivo == 0;
    loop {
        let ahora = dashos::ahora_us();
        if ahora >= fin {
            return None;
        }
        match aviso(fin - ahora) {
            Some((C_LISTO, g, _)) if cayo => return Some(g),
            Some((C_CAIDO, g, m)) => {
                println!("  [dominio-prueba] la generación {} del dominio cae (motivo {})", g, m);
                cayo |= m == motivo;
            }
            _ => {}
        }
    }
}

fn relanzar(al_servicio: u32, modo: &str) {
    let mut m = [0u8; 52];
    m[..4].copy_from_slice(&C_RELANZAR.to_le_bytes());
    let n = modo.len().min(48);
    m[4..4 + n].copy_from_slice(&modo.as_bytes()[..n]);
    dashos::canal_escribir(al_servicio, &m[..4 + n], &[]);
}

fn principal() -> u64 {
    // El HOLA del servicio del dominio, con la llave para pedirle cosas.
    // `inicio` lanza el servicio del dominio el último: puede tardar minutos con QEMU sin aceleración. Por el canal llega
    // también lo que diga `inicio` de sus clientes: se ignora.
    let mut b = [0u8; 256];
    let mut ll = [0u32; 4];
    let hasta = dashos::ahora_us() + 900_000_000;
    let al_servicio = loop {
        let (n, k, _) = dashos::canal_leer_plazo(CONTROL, &mut b, &mut ll, 1_000_000);
        if n >= 4 && k == 1 && u32::from_le_bytes(b[..4].try_into().unwrap_or([0; 4])) == C_HOLA {
            break ll[0];
        }
        if n >= 0 {
            ll[..k.min(ll.len())].iter().for_each(|&l| {
                dashos::cerrar(l);
            });
        }
        if dashos::ahora_us() > hasta || (n < 0 && n != dashos::E_PLAZO) {
            println!("  [dominio-prueba] el servicio del dominio no saluda ({}); MAL", n);
            return 1;
        }
    };
    let mut bien = true;

    // 1. Normal.
    let t0 = dashos::ahora_us();
    let Some(g) = esperar_vuelta(0, 120_000_000) else {
        println!("  [dominio-prueba] el dominio no tiene aparato en 120 s; MAL");
        return 2;
    };
    let e = loop {
        match estado(SERVICIO, 2_000_000) {
            Ok(e) if e[p::E_CONFIGURADA] == 1 => break e,
            _ if dashos::ahora_us() - t0 > 120_000_000 => {
                println!("  [dominio-prueba] sin dirección por DHCP en 120 s; MAL");
                return 3;
            }
            _ => dashos::dormir_us(200_000),
        }
    };
    let a = e[p::E_IP].to_le_bytes();
    println!("  [dominio-prueba] DHCP por el dominio: {}.{}.{}.{} a los {} ms", a[0], a[1], a[2], a[3], (dashos::ahora_us() - t0) / 1000);
    bien &= eco_tcp("normal", 64 * 1024, 1, 60_000_000);
    bien &= eco_udp();

    // 2-4. Hostiles. En las dos últimas, el eco va a la vez que el dominio se cae: TCP lo reintenta y acaba cuando el
    // dominio vuelve.
    let _ = g;
    for (modo, total, nombre, cae) in [
        ("oferta,descriptores,fuera", 64 * 1024, "hostil, oferta y descriptores", 0),
        ("colgarse", 256 * 1024, "hostil, se cuelga", M_COLGADO),
        ("morir", 256 * 1024, "hostil, muere", M_REINICIO),
    ] {
        relanzar(al_servicio, modo);
        let Some(g) = esperar_vuelta(M_ORDEN, 120_000_000) else {
            println!("  [dominio-prueba] {}: el dominio no vuelve en 120 s; MAL", nombre);
            bien = false;
            break;
        };
        println!("  [dominio-prueba] {}: generación {} del dominio en marcha", nombre, g);
        bien &= eco_tcp(nombre, total, g as u64, 120_000_000);
        if cae == 0 {
            bien &= eco_udp();
        } else {
            // El dominio tenía que caer a media transferencia, y volver.
            match esperar_vuelta(cae, 60_000_000) {
                Some(n) => println!("  [dominio-prueba] {}: el dominio cayó a media transferencia y volvió (generación {}): bien", nombre, n),
                None => {
                    println!("  [dominio-prueba] {}: el dominio no cayó como se esperaba o no volvió; MAL", nombre);
                    bien = false;
                }
            }
        }
    }
    // Tras los hostiles, el dominio sigue sirviendo.
    bien &= eco_tcp("al final", 16 * 1024, 99, 60_000_000);
    println!("  [dominio-prueba] el dominio de red de punta a punta: {}", if bien { "bien" } else { "MAL" });
    0
}
