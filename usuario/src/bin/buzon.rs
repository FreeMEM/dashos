//! El buzón de la VideoCore como servicio (C50). Antes iba dentro del kernel (C15).
//!
//! Llaves: la 1, su permiso DMA; la 2, los registros del buzón (la página entera: el buzón está en su desplazamiento
//! 0x880, en todas las Pi); la 3, la memoria de la VideoCore; la 4, el canal por el que llegan las peticiones.
//!
//! Cada petición es un mensaje de propiedades del firmware tal cual (palabras de 32 bits en little-endian: tamaño,
//! código 0, etiquetas y la etiqueta 0 de fin), de hasta 256 bytes, y lleva una llave: un extremo de canal por el que
//! va la respuesta, del mismo tamaño. Si el servicio no acepta la petición, la respuesta lleva el código 0x80000001 y el
//! firmware no la ve; sin llave de respuesta, no se contesta.
//!
//! Quién pide qué (auditoría de la fase 4, M6): cada cliente escribe con una copia marcada del canal, y `inicio` manda
//! antes, sin marca, un mensaje de directorio por cliente (8 bytes de marca y 16 de nombre). Las etiquetas de leer
//! valen para todos; las de la pantalla, solo para el "compositor"; encender y poner el reloj de la V3D, solo para la
//! "gpu". Al reservar la pantalla, la respuesta lleva una segunda llave: la memoria de la VideoCore recortada a la
//! pantalla (M4). Una sola vez: después ya no se deja reservar otra ni cambiar su tamaño, para que el compositor no vaya
//! juntando trozos de esa memoria con pantallas que el firmware mueve (M8).

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

/// Reloj de la V3D (`docs/spec/hw/v3d.md`, §2.3): el único que se deja encender y poner.
const RELOJ_V3D: u32 = 5;
const PONER_ESTADO_RELOJ: u32 = 0x0003_8001;
const PONER_FRECUENCIA: u32 = 0x0003_8002;

const RESERVAR_PANTALLA: u32 = 0x0004_0001;

/// Lo que puede pedir cualquiera: leer relojes.
const DE_TODOS: &[u32] = &[
    0x0003_0002, // frecuencia de un reloj
    0x0003_0004, // frecuencia máxima de un reloj
];
/// Solo el compositor: la pantalla.
const DEL_COMPOSITOR: &[u32] = &[
    RESERVAR_PANTALLA,
    0x0004_0008, // paso de línea
    0x0004_8003, // tamaño físico
    0x0004_8004, // tamaño virtual
    0x0004_8005, // bits por píxel
    0x0004_8006, // orden de los colores
    0x0004_8009, // desplazamiento virtual
];
/// Solo la GPU: encender su reloj y ponerle la frecuencia (y solo el suyo, se mira en `valido`).
const DE_LA_GPU: &[u32] = &[PONER_ESTADO_RELOJ, PONER_FRECUENCIA];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cliente {
    Otro,
    Compositor,
    Gpu,
}

/// Una vez dada la pantalla, esto ya no se deja: reservar otra y cambiar el tamaño (M8).
const TRAS_LA_PANTALLA: &[u32] = &[RESERVAR_PANTALLA, 0x0004_8003, 0x0004_8004, 0x0004_8005];

static PANTALLA_DADA: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn permitida(c: Cliente, etiqueta: u32) -> bool {
    if PANTALLA_DADA.load(core::sync::atomic::Ordering::Relaxed) && TRAS_LA_PANTALLA.contains(&etiqueta) {
        return false;
    }
    DE_TODOS.contains(&etiqueta)
        || (c == Cliente::Compositor && DEL_COMPOSITOR.contains(&etiqueta))
        || (c == Cliente::Gpu && DE_LA_GPU.contains(&etiqueta))
}

/// Espera un tic sin gastar CPU (un temporizador de una vez).
fn esperar_un_tic() {
    let (a, b) = dashos::canal_crear();
    let t = dashos::temporizador_crear(b, 10_000, 0);
    if t > 0 {
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        dashos::canal_leer(a, &mut buf, &mut ll);
        dashos::cerrar(t as u32);
    }
    dashos::cerrar(a);
    dashos::cerrar(b);
}

/// Dónde empieza el buzón 0 dentro de su página (0x7E00B880 en el bus de la VideoCore).
const DESPLAZAMIENTO: usize = 0x880;

fn reg(off: usize) -> *mut u32 {
    (VA_REGISTROS + DESPLAZAMIENTO + off) as *mut u32
}

fn leer(off: usize) -> u32 {
    u32::from_le(unsafe { core::ptr::read_volatile(reg(off)) })
}

/// ¿Es un mensaje bien formado y con solo etiquetas permitidas a ese cliente? `p` son las palabras, ya en el orden de la
/// CPU.
fn valido(c: Cliente, p: &[u32]) -> bool {
    if p.len() < 3 || p[0] as usize != p.len() * 4 || p[1] != 0 {
        return false;
    }
    let mut i = 2;
    let mut reservada = false;
    while i < p.len() {
        let etiqueta = p[i];
        if etiqueta == 0 {
            return true; // lo que haya detrás es relleno
        }
        // Tras reservar la pantalla, en el mismo mensaje, ni otra reserva ni cambios de tamaño (tercera vuelta, B19).
        if reservada && TRAS_LA_PANTALLA.contains(&etiqueta) {
            return false;
        }
        reservada |= etiqueta == RESERVAR_PANTALLA;
        if i + 3 > p.len() || !permitida(c, etiqueta) {
            return false;
        }
        let palabras = (p[i + 1] as usize).div_ceil(4);
        // Poner un reloj, solo el de la V3D: el primer valor es el número de reloj.
        if (etiqueta == PONER_ESTADO_RELOJ || etiqueta == PONER_FRECUENCIA) && (palabras == 0 || i + 3 >= p.len() || p[i + 3] != RELOJ_V3D) {
            return false;
        }
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

/// La memoria de la VideoCore según el firmware: (base física, tamaño).
fn memoria_vc(mensaje: *mut u32, bus: u64) -> Option<(u32, u32)> {
    let p = [32u32, 0, 0x0001_0006, 8, 0, 0, 0, 0];
    for (i, &w) in p.iter().enumerate() {
        unsafe { mensaje.add(i).write_volatile(w.to_le()) };
    }
    if !mandar(bus) {
        return None;
    }
    let leer = |i: usize| u32::from_le(unsafe { mensaje.add(i).read_volatile() });
    (leer(1) == 0x8000_0000).then(|| (leer(5), leer(6)))
}

const E_CERRADO: i64 = -4;
const MAX_CLIENTES: usize = 16;

fn principal() -> u64 {
    if dashos::registros_proyectar(2, VA_REGISTROS) != 0 {
        dashos::println!("  [buzón] sin registros");
        return 1;
    }
    // Lo que dejara en cuarentena un buzón anterior: el buzón no tiene DMA que siga en marcha (M5).
    dashos::dma_liberar(1);
    // El mensaje va en memoria DMA del primer GB, sin caché: la VideoCore lo lee y lo escribe sin limpiar cachés.
    let (m, bus) = dashos::memoria_dma(1, 4096);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_MENSAJE, true) != 0 {
        dashos::println!("  [buzón] sin memoria DMA");
        return 2;
    }
    let mensaje = VA_MENSAJE as *mut u32;
    let vc = memoria_vc(mensaje, bus);
    let mut clientes = [(0u64, Cliente::Otro); MAX_CLIENTES];
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 8];
    loop {
        let (n, k, marca) = dashos::canal_leer_marca(4, &mut buf, &mut ll);
        if n == E_CERRADO {
            return 0; // los clientes cerraron el canal
        }
        if n < 0 {
            // Sin sitio en la tabla o sin memoria: el mensaje vuelve a la cola. Se espera un tic antes de intentarlo
            // otra vez, para no girar (B16).
            esperar_un_tic();
            continue;
        }
        // Las llaves de más, fuera.
        for &l in ll.iter().take(k).skip(1) {
            dashos::cerrar(l);
        }
        let n = n as usize;
        // Un mensaje sin marca y sin llaves es el directorio de `inicio`: qué cliente lleva cada marca.
        if marca == 0 && k == 0 && n == 24 {
            let m = u64::from_le_bytes(buf[..8].try_into().unwrap());
            let nombre = &buf[8..24];
            let c = if nombre.starts_with(b"compositor\0") {
                Cliente::Compositor
            } else if nombre.starts_with(b"gpu\0") {
                Cliente::Gpu
            } else {
                Cliente::Otro
            };
            if let Some(hueco) = clientes.iter_mut().find(|x| x.0 == 0 || x.0 == m) {
                *hueco = (m, c);
            }
            continue;
        }
        if k == 0 {
            continue; // sin canal de respuesta
        }
        let respuesta = ll[0];
        let cliente = if marca == 0 { Cliente::Otro } else { clientes.iter().find(|x| x.0 == marca).map_or(Cliente::Otro, |x| x.1) };
        let mut p = [0u32; 64];
        let palabras = n / 4;
        for (i, w) in p.iter_mut().enumerate().take(palabras) {
            *w = u32::from_le_bytes(buf[4 * i..4 * i + 4].try_into().unwrap());
        }
        let p = &mut p[..palabras];
        let mut pantalla = None;
        if n % 4 != 0 || !valido(cliente, p) {
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
                pantalla = recortar_pantalla(p, vc);
            } else {
                p[1] = RECHAZADO;
            }
        }
        for (i, &w) in p.iter().enumerate() {
            buf[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
        }
        match pantalla {
            Some(l) => {
                PANTALLA_DADA.store(true, core::sync::atomic::Ordering::Relaxed);
                if dashos::canal_escribir(respuesta, &buf[..n], &[l]) != 0 {
                    dashos::cerrar(l);
                }
            }
            None => {
                dashos::canal_escribir(respuesta, &buf[..n], &[]);
            }
        }
        dashos::cerrar(respuesta);
    }
}

/// Si la respuesta reservó la pantalla, una llave de la memoria de la VideoCore recortada a ella (M4). Solo si el
/// firmware contestó bien al mensaje y a esa etiqueta, con 8 bytes de respuesta: si no, los valores serían los que
/// puso el cliente (segunda vuelta de la auditoría, M8).
fn recortar_pantalla(p: &[u32], vc: Option<(u32, u32)>) -> Option<u32> {
    let (base, tamano_vc) = vc?;
    if p.len() < 2 || p[1] != 0x8000_0000 {
        return None;
    }
    let mut i = 2;
    while i + 3 <= p.len() && p[i] != 0 {
        let palabras = (p[i + 1] as usize).div_ceil(4);
        if p[i] == RESERVAR_PANTALLA && i + 4 < p.len() && p[i + 2] == 0x8000_0008 && p[i + 3] != 0 {
            let fisica = (p[i + 3] & 0x3fff_ffff) as u64;
            let tamano = p[i + 4] as u64;
            let desde = (fisica.checked_sub(base as u64)?) & !0xfff;
            let hasta = (fisica - base as u64 + tamano).next_multiple_of(4096);
            if hasta > tamano_vc as u64 {
                return None;
            }
            let l = dashos::registros_recortar(3, desde, hasta - desde);
            return (l > 0).then_some(l as u32);
        }
        i += 3 + palabras;
    }
    None
}
