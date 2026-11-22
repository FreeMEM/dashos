//! La prueba de punta a punta del dominio de red (S35, paso 2; `docs/bitacoras/dominio-red.md` y `dominio-usb.md`): la
//! red de DashOS sale por un adaptador de red USB que el xHCI de DashOS presta (por USB/IP) a un Linux en un dominio,
//! que lo lleva con su controlador y que DashOS usa como una virtio-net.
//!
//! Por fases, cada una con el dominio en un modo (lo pide por "dominio-control" al servicio `dominio`, que lo relanza):
//!
//! 1. **Normal**: DHCP (de la red de usuario de QEMU, en el mismo concentrador que el adaptador), una conexión TCP con
//!    el eco del PC (10.0.2.100:7, por la red de QEMU, sin salir de él) y el eco UDP del vecino de prueba
//!    (`tools/red_vecino.py`, 10.0.2.53:4007). Nunca Internet.
//! 2. **Hostil: virtio y USB**: el dominio miente en su oferta, devuelve usados que no son del conductor, lanza una
//!    tormenta de avisos, intenta leer fuera de su memoria y, por USB/IP, pide aparatos que no se le prestaron, manda
//!    peticiones falsas o enormes y una tormenta de miles de peticiones y cancelaciones (y el servicio pide al xHCI
//!    aparatos que no le ofreció). La red sigue: otro eco TCP.
//! 3. **Hostil: basura**: una orden de USB/IP que no existe: el dominio se da por roto y se relanza.
//! 4. **Se desenchufa**: con un eco TCP en marcha, pide al guion (`tools/guion_dominio_usb.py`) que quite el adaptador
//!    y, unos segundos después, que lo ponga otra vez: el eco acaba.
//! 5. **Hostil: se cuelga** y **6. muere** a media transferencia, con el aparato prestado: el vigilante lo relanza, el
//!    aparato vuelve limpio al xHCI y se presta otra vez, y el eco acaba.
//! 7. **Se cae el conductor**: mata a `virtio-red` (por `sesiones`); `inicio` lo relanza y el servicio se le vuelve a
//!    presentar: la red sigue.
//! 8. **Tres caídas seguidas**: el dominio se cae nada más arrancar, tres veces: aviso al usuario.
//! 9. **Los registros**: cada caída del dominio quedó en `Estado:Registros/sistema` y su consola en
//!    `Estado:Registros/dominio-red` (se leen por `sesiones`).
//!
//! Llaves (por nombre): "red" (el permiso de red), "dominio-control" (lo que avisa el servicio del dominio) y
//! "sesiones" (leer el registro común y matar al conductor).

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::red as p;
use dashos::red_cliente::{estado, Conexion, Udp};

dashos::programa!(principal);

static SERVICIO: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static CONTROL: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

fn servicio() -> u32 {
    SERVICIO.load(core::sync::atomic::Ordering::Relaxed)
}
fn control() -> u32 {
    CONTROL.load(core::sync::atomic::Ordering::Relaxed)
}
const VA_REGISTRO: usize = 0x60_0000_0000;
const VA: usize = 0x50_0000_0000;
const VA2: usize = VA + (4 << 20);
const ECO: [u8; 4] = [10, 0, 2, 100];
const VECINO: [u8; 4] = [10, 0, 2, 53];
const P_ECO_VECINO: u16 = 4007;

const C_HOLA: u32 = 1;
const C_RELANZAR: u32 = 2;
const C_LISTO: u32 = 3;
const C_CAIDO: u32 = 4;
const C_RELANZAR_VECES: u32 = 5;
const C_AVISO: u32 = 6;

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

/// Manda `total` bytes por `c` y comprueba que vuelven iguales. Microsegundos que tardó, o qué falló. Con `a_medias`,
/// al llegar la mitad de vuelta la llama una vez (desenchufar el adaptador).
fn eco_con(c: &Conexion, total: usize, semilla: u64, plazo_us: u64, mut a_medias: Option<&mut dyn FnMut()>) -> Result<u64, &'static str> {
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
            if recibidos >= total / 2 {
                if let Some(f) = a_medias.take() {
                    f();
                }
            }
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
    eco_tcp_con(fase, total, semilla, plazo_us, None)
}

fn eco_tcp_con(fase: &str, total: usize, semilla: u64, plazo_us: u64, a_medias: Option<&mut dyn FnMut()>) -> bool {
    match Conexion::conectar(servicio(), ECO, 7, VA, 64 * 1024, 20_000_000) {
        Ok(c) => {
            let r = eco_con(&c, total, semilla, plazo_us, a_medias);
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
    let Ok(z) = Udp::abrir(servicio(), 0, VA2, 16 * 1024, 5_000_000) else {
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
    let (n, k, _) = dashos::canal_leer_plazo(control(), &mut b, &mut ll, plazo_us);
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
const M_PROTOCOLO: u32 = 9;

/// Espera a que caiga la generación del dominio que está en marcha por `motivo` (cualquiera, si es 0) y a que la
/// siguiente tenga su aparato listo: el número de esta.
fn esperar_vuelta(motivo: u32, plazo_us: u64) -> Option<u32> {
    esperar_vuelta_viendo(motivo, plazo_us).map(|x| x.0)
}

/// Como `esperar_vuelta`, y además los motivos de las caídas que vio entre medias (un bit por motivo).
fn esperar_vuelta_viendo(motivo: u32, plazo_us: u64) -> Option<(u32, u32)> {
    let fin = dashos::ahora_us() + plazo_us;
    let mut cayo = motivo == 0;
    let mut vistos = 0u32;
    loop {
        let ahora = dashos::ahora_us();
        if ahora >= fin {
            return None;
        }
        match aviso(fin - ahora) {
            Some((C_LISTO, g, _)) if cayo => return Some((g, vistos)),
            Some((C_CAIDO, g, m)) => {
                println!("  [dominio-prueba] la generación {} del dominio cae (motivo {})", g, m);
                cayo |= m == motivo;
                vistos |= 1 << (m & 31);
            }
            Some((C_AVISO, n, _)) => println!("  [dominio-prueba] el servicio avisa al usuario tras {} caídas seguidas", n),
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

/// Relanza el dominio en `modo` durante `veces` generaciones seguidas.
fn relanzar_veces(al_servicio: u32, modo: &str, veces: u32) {
    let mut m = [0u8; 56];
    m[..4].copy_from_slice(&C_RELANZAR_VECES.to_le_bytes());
    m[4..8].copy_from_slice(&veces.to_le_bytes());
    let n = modo.len().min(48);
    m[8..8 + n].copy_from_slice(&modo.as_bytes()[..n]);
    dashos::canal_escribir(al_servicio, &m[..8 + n], &[]);
}

/// Una petición a `sesiones` con una llave para contestar; lo que contesta.
fn pedir_a_sesiones(sesiones: u32, orden: u32, objetivo: u64, buf: &mut [u8]) -> i64 {
    let (lectura, suyo) = dashos::canal_crear();
    let mut p = [0u8; 12];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..12].copy_from_slice(&objetivo.to_le_bytes());
    if dashos::canal_escribir(sesiones, &p, &[suyo]) != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return -1;
    }
    let n = dashos::canal_leer_plazo(lectura, buf, &mut [], 5_000_000).0;
    dashos::cerrar(lectura);
    n
}

/// Mata al conductor (`virtio-red`) por `sesiones` (los dos son del sistema). Devuelve el estado.
fn matar_conductor(sesiones: u32) -> Result<u64, i64> {
    use dashos::protocolo::sesiones as ps;
    let mut desde = 0u64;
    let mut marca = None;
    for _ in 0..32 {
        let mut b = [0u8; 256];
        let n = pedir_a_sesiones(sesiones, ps::PROCESOS, desde, &mut b);
        if n < 8 {
            return Err(n);
        }
        let van = u32::from_le_bytes(b[0..4].try_into().unwrap_or([0; 4])) as usize;
        let total = u32::from_le_bytes(b[4..8].try_into().unwrap_or([0; 4])) as u64;
        for i in 0..van.min(ps::POR_PAGINA) {
            let f = &b[8 + ps::FICHA * i..8 + ps::FICHA * (i + 1)];
            if &f[16..26] == b"virtio-red" && f[26] == 0 {
                marca = Some(u64::from_le_bytes(f[..8].try_into().unwrap_or([0; 8])));
            }
        }
        desde += van as u64;
        if van == 0 || desde >= total {
            break;
        }
    }
    let m = marca.ok_or(-2i64)?;
    let mut r = [0u8; 4];
    let n = pedir_a_sesiones(sesiones, ps::MATAR, m, &mut r);
    if n == 4 && i32::from_le_bytes(r) == 0 { Ok(m) } else { Err(if n == 4 { i32::from_le_bytes(r) as i64 } else { n }) }
}

/// Las líneas del fichero `fichero` del registro común que cumplen `vale`: cuántas, o el error. Se lee por trozos (cada
/// respuesta dice hasta dónde llegó), desde el principio.
fn lineas_del_registro(sesiones: u32, fichero: usize, vale: impl Fn(&str) -> bool) -> Result<usize, i64> {
    use dashos::protocolo::sesiones as ps;
    let mut cuantas = 0;
    let mut desde = 0u64;
    for _ in 0..256 {
        let (lectura, suyo) = dashos::canal_crear();
        let mut m = [0u8; 20];
        m[0..4].copy_from_slice(&ps::REGISTRO_LEER.to_le_bytes());
        m[4..8].copy_from_slice(&(fichero as u32).to_le_bytes());
        m[12..20].copy_from_slice(&desde.to_le_bytes());
        let r = dashos::canal_escribir(sesiones, &m, &[suyo]);
        if r != 0 {
            dashos::cerrar(lectura);
            dashos::cerrar(suyo);
            return Err(r);
        }
        let mut b = [0u8; ps::LARGO_REGISTRO_LEER];
        let mut ll = [0u32; 1];
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut b, &mut ll, 5_000_000);
        dashos::cerrar(lectura);
        let estado = i32::from_le_bytes(b[..4].try_into().unwrap_or([0; 4]));
        if n < 0 || estado != 0 {
            return Err(if n < 0 { n } else { estado as i64 });
        }
        let hasta = u64::from_le_bytes(b[8..16].try_into().unwrap_or([0; 8]));
        let largo = u64::from_le_bytes(b[16..24].try_into().unwrap_or([0; 8])) as usize;
        if k == 1 {
            if largo > 0 && dashos::memoria_proyectar(ll[0], VA_REGISTRO, false) == 0 {
                // SAFETY: recién proyectada aquí, solo para leer; mide al menos `largo` (lo que dice `sesiones`).
                let texto = unsafe { core::slice::from_raw_parts(VA_REGISTRO as *const u8, largo) };
                cuantas += texto
                    .split(|&c| c == b'\n')
                    .filter(|l| vale(core::str::from_utf8(l).unwrap_or("")))
                    .count();
                dashos::memoria_quitar(VA_REGISTRO);
            }
            dashos::cerrar(ll[0]);
        }
        if hasta <= desde {
            break;
        }
        desde = hasta;
    }
    Ok(cuantas)
}

fn principal() -> u64 {
    let (Some(red), Some(ctl), Some(ses)) = (dashos::llaves::buscar("red"), dashos::llaves::buscar("dominio-control"), dashos::llaves::buscar("sesiones")) else {
        println!("  [dominio-prueba] le faltan llaves; MAL");
        return 1;
    };
    SERVICIO.store(red, core::sync::atomic::Ordering::Relaxed);
    CONTROL.store(ctl, core::sync::atomic::Ordering::Relaxed);
    // El HOLA del servicio del dominio, con la llave para pedirle cosas.
    // `inicio` lanza el servicio del dominio el último: puede tardar minutos con QEMU sin aceleración. Por el canal llega
    // también lo que diga `inicio` de sus clientes: se ignora.
    let mut b = [0u8; 256];
    let mut ll = [0u32; 4];
    let hasta = dashos::ahora_us() + 900_000_000;
    let al_servicio = loop {
        let (n, k, _) = dashos::canal_leer_plazo(control(), &mut b, &mut ll, 1_000_000);
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
        match estado(servicio(), 2_000_000) {
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

    // 2-6. Hostiles. En los de caer, el eco va a la vez que el dominio se cae: TCP lo reintenta y acaba cuando el
    // dominio vuelve (con el aparato devuelto y prestado otra vez).
    let _ = g;
    for (modo, total, nombre, cae) in [
        ("oferta,descriptores,fuera,ajeno,usbmalo,tormenta", 64 * 1024, "hostil, virtio y USB", 0),
        ("basura", 16 * 1024, "hostil, basura en USB/IP", M_PROTOCOLO),
        ("", 256 * 1024, "se desenchufa", 0),
        ("colgarse", 256 * 1024, "hostil, se cuelga", M_COLGADO),
        ("morir", 256 * 1024, "hostil, muere", M_REINICIO),
    ] {
        relanzar(al_servicio, modo);
        let Some((g, vistos)) = esperar_vuelta_viendo(M_ORDEN, 120_000_000) else {
            println!("  [dominio-prueba] {}: el dominio no vuelve en 120 s; MAL", nombre);
            bien = false;
            break;
        };
        println!("  [dominio-prueba] {}: generación {} del dominio en marcha", nombre, g);
        if modo == "basura" {
            // Se rompe nada más arrancar (antes de estar listo) y vuelve normal: la generación que está en marcha ya es
            // la siguiente. Si aún no se vio caer por el protocolo, se espera.
            let n = if vistos & (1 << M_PROTOCOLO) != 0 { Some(g) } else { esperar_vuelta(M_PROTOCOLO, 60_000_000) };
            match n {
                Some(n) => println!("  [dominio-prueba] {}: el dominio rompió el protocolo, se relanzó (generación {}): bien", nombre, n),
                None => {
                    println!("  [dominio-prueba] {}: el dominio no cayó por el protocolo o no volvió; MAL", nombre);
                    bien = false;
                }
            }
            bien &= eco_tcp(nombre, total, g as u64, 120_000_000);
            continue;
        }
        if nombre == "se desenchufa" {
            let mut pausa = || {
                println!("  [dominio-prueba] pide desenchufar el adaptador USB");
                // Desenchufado 2 s: con lo que tarda en volver (enumerarlo, prestarlo, que Linux lo engancha), el corte
                // queda por debajo de los 10 s sin respuesta con que el servicio de red da por muerta una conexión TCP.
                dashos::dormir_us(2_000_000);
                println!("  [dominio-prueba] pide enchufar el adaptador USB");
            };
            let t = dashos::ahora_us();
            let r = eco_tcp_con(nombre, total, g as u64, 180_000_000, Some(&mut pausa));
            if r {
                println!("  [dominio-prueba] {}: el adaptador se fue a media transferencia y volvió; el eco acabó a los {} ms: bien", nombre, (dashos::ahora_us() - t) / 1000);
            }
            bien &= r;
            continue;
        }
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
    // 7. Se cae el conductor: `inicio` lo relanza y el servicio del dominio se le vuelve a presentar.
    match matar_conductor(ses) {
        Ok(m) => {
            println!("  [dominio-prueba] mata al conductor (virtio-red, marca {}): inicio lo relanza", m);
            let t = dashos::ahora_us();
            let mut vuelve = false;
            while dashos::ahora_us() - t < 60_000_000 && !vuelve {
                dashos::dormir_us(1_000_000);
                // Sin decir nada mientras no vuelve: conectar falla hasta que hay tarjeta.
                vuelve = matches!(estado(servicio(), 2_000_000), Ok(e) if e[p::E_CONFIGURADA] == 1)
                    && Conexion::conectar(servicio(), ECO, 7, VA, 64 * 1024, 5_000_000).map(|c| c.cerrar()).is_ok();
            }
            if vuelve && eco_tcp("conductor relanzado", 16 * 1024, 77, 30_000_000) {
                println!("  [dominio-prueba] la red vuelve con el conductor relanzado a los {} ms: bien", (dashos::ahora_us() - t) / 1000);
            } else {
                println!("  [dominio-prueba] la red no vuelve con el conductor relanzado; MAL");
                bien = false;
            }
        }
        Err(e) => {
            println!("  [dominio-prueba] no se puede matar al conductor ({}); MAL", e);
            bien = false;
        }
    }
    // 8. Tres caídas seguidas (el dominio se cae nada más arrancar): el aviso al usuario. Antes, un minuto: con las
    // caídas de las fases de antes (colgarse, morir) serían más de 5 en un minuto y el servicio lo dejaría apagado ("cae
    // sin parar"), que es otra cosa.
    dashos::dormir_us(61_000_000);
    relanzar_veces(al_servicio, "caer", 3);
    let t = dashos::ahora_us();
    let mut caidas = 0;
    let mut avisado = false;
    while dashos::ahora_us() - t < 180_000_000 && !(avisado && caidas >= 3) {
        match aviso(5_000_000) {
            Some((C_CAIDO, g, m)) => {
                println!("  [dominio-prueba] tres seguidas: la generación {} cae (motivo {})", g, m);
                if m == M_REINICIO {
                    caidas += 1;
                }
            }
            Some((C_AVISO, n, _)) => {
                println!("  [dominio-prueba] tres seguidas: el servicio avisa al usuario tras {} caídas seguidas", n);
                avisado = true;
            }
            _ => {}
        }
    }
    if avisado && caidas >= 3 {
        println!("  [dominio-prueba] tres caídas seguidas y el aviso: bien");
    } else {
        println!("  [dominio-prueba] tres seguidas: {} caídas, aviso {}; MAL", caidas, avisado);
        bien = false;
    }
    if esperar_vuelta(0, 120_000_000).is_none() {
        println!("  [dominio-prueba] tras las caídas seguidas el dominio no vuelve; MAL");
        bien = false;
    }
    // Tras todo, el dominio sigue sirviendo.
    bien &= eco_tcp("al final", 16 * 1024, 99, 60_000_000);
    // 9. Sus caídas quedaron en el registro común, y su consola en el suyo.
    match lineas_del_registro(ses, dashos::registro::SISTEMA, |s| s.contains(" dominio ") && s.contains("el dominio de red cae")) {
        Ok(n) if n >= 10 => println!("  [dominio-prueba] el registro común tiene {} caídas del dominio: bien", n),
        Ok(n) => {
            println!("  [dominio-prueba] el registro común tiene {} caídas del dominio (se esperaban 10 o más); MAL", n);
            bien = false;
        }
        Err(e) => {
            println!("  [dominio-prueba] no se puede leer el registro común ({}); MAL", e);
            bien = false;
        }
    }
    let consola = lineas_del_registro(ses, dashos::registro::DOMINIO_RED, |s| s.contains(" consola ") && s.contains("dominio-red: en marcha"));
    let reinicios = lineas_del_registro(ses, dashos::registro::DOMINIO_RED, |s| s.contains(" consola ") && s.contains("--- la generación"));
    match (consola, reinicios) {
        (Ok(a), Ok(r)) if a >= 10 && r >= 10 => {
            println!("  [dominio-prueba] el registro del dominio (dominio-red) tiene su consola ({} arranques) y {} reinicios: bien", a, r)
        }
        (a, r) => {
            println!("  [dominio-prueba] el registro del dominio no tiene lo esperado ({:?} arranques, {:?} reinicios); MAL", a, r);
            bien = false;
        }
    }
    println!("  [dominio-prueba] el dominio de red de punta a punta: {}", if bien { "bien" } else { "MAL" });
    0
}
