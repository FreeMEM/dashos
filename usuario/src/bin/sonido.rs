//! El servicio de sonido (S19, A4): mezcla los flujos de los programas y entrega períodos al controlador.
//!
//! Por el canal "sonido" le llega todo (un proceso solo espera en un canal): los mensajes de `inicio` (sin marca: qué
//! marca es de quién), los del controlador y los de los programas, cada uno con su marca. El protocolo, en
//! `docs/referencia/servicios.md`:
//!
//! - El controlador (solo uno de los que `inicio` llama "virtio-sonido" o "hda", X5c; el primero) manda `SALIDA` con una memoria compartida: cabecera
//!   (escritos, leídos, ranuras, cuadros por período) y las ranuras desde el byte 64, cada una un período en estéreo de
//!   16 bits. Después, un `TIC` cada vez que el aparato devuelve un búfer: el reloj del hardware manda (S19). Con cada
//!   tic, el servicio mezcla hasta tener `ADELANTE` períodos listos.
//! - Un programa manda `ABRIR` (formato) con la memoria de su anillo y el extremo de escritura de un canal suyo
//!   (`usuario/src/sonido_cliente.rs`); `VOLUMEN` y `CERRAR`. Un flujo por programa.
//! - `GENERAL` (f32): el volumen general, sobre la mezcla entera. Solo del panel rápido (C2 de S10c); de otro, nada.
//!
//! U6, el cambio rápido de usuario (`docs/diseño/U6-cambio-rapido.md`): `sesiones` manda, sin marca (`AVISO_PERSONA`,
//! `AVISO_DETRAS` de `dashos::protocolo::sesiones`), de qué persona es cada programa y qué sesiones están detrás. Un
//! flujo de una sesión de detrás sigue leyéndose a su ritmo (al programa no se le llena el anillo ni se le cuelga),
//! pero se mezcla con volumen 0: no suena. Al volver delante, otra vez con su volumen.
//!
//! Cuando el anillo de un programa baja de la mitad, se le despierta por su canal. Si el canal está cerrado, el
//! programa murió: su flujo sale con el fundido del mezclador (A5).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};
use dashos::println;
use sonido::{Formato, Mezclador, Muestra};

dashos::programa!(principal);
dashos::monton!();

const SALIDA: u32 = 1;
const TIC: u32 = 2;
const ABRIR: u32 = 10;
const VOLUMEN: u32 = 11;
const CERRAR: u32 = 12;
const GENERAL: u32 = 13;
/// Los controladores, por el nombre que da `inicio`: el primero que manda `SALIDA` es el que suena (X5c).
const CONTROLADORES: [&str; 2] = ["virtio-sonido", "hda"];
/// Quién puede cambiar el volumen general, por el nombre que da `inicio`.
const PONEN_GENERAL: &[&str] = &["panel"];
const E_CERRADO: i64 = -4;

const VA_SALIDA: usize = 0x40_0000_0000;
const VA_ANILLOS: usize = 0x41_0000_0000;
/// Sitio de cada anillo: 1 MB.
const HUECO: usize = 1 << 20;
const DATOS: usize = 64;
/// Períodos mezclados por delante del controlador (4 x 5,3 ms).
const ADELANTE: u32 = 4;
/// Cuadros que se procuran tener en la cola del mezclador de cada flujo antes de mezclar.
const RESERVA: usize = 1024;

fn palabra(va: usize, i: usize) -> u32 {
    unsafe { read_volatile((va + 4 * i) as *const u32) }
}

fn poner(va: usize, i: usize, v: u32) {
    unsafe { write_volatile((va + 4 * i) as *mut u32, v) }
}

struct Cliente {
    nombre: String,
    /// El volumen que pidió el programa (U6: se guarda mientras suena en silencio).
    volumen: (f32, f32),
    /// U6: en silencio porque su sesión está detrás, y lo que se leyó de su anillo así (bytes).
    silencio: bool,
    leido_en_silencio: u64,
    id: u32,
    va: usize,
    tam: u32,
    aviso: u32,
    memoria: u32,
    avisado: bool,
    ultima: u32,
    cerrando: bool,
    faltas: u64,
}

struct Salida {
    ranuras: u32,
    cuadros: u32,
}

/// Lo que hay en el anillo (sin fiarse de la cabecera, que escribe el programa).
fn disponibles(c: &Cliente) -> u32 {
    palabra(c.va, 0).wrapping_sub(palabra(c.va, 1)).min(c.tam)
}

/// Pasa al mezclador lo que haga falta del anillo del programa.
fn recoger(m: &mut Mezclador, c: &mut Cliente, tmp: &mut Vec<u8>) {
    if m.pendientes(c.id).unwrap_or(usize::MAX) >= RESERVA {
        return;
    }
    let n = disponibles(c).min(4096) as usize;
    if n == 0 {
        return;
    }
    fence(Ordering::SeqCst);
    let cola = palabra(c.va, 1);
    tmp.clear();
    for i in 0..n {
        let p = (cola as usize + i) & (c.tam as usize - 1);
        tmp.push(unsafe { read_volatile((c.va + DATOS + p) as *const u8) });
    }
    let usados = m.escribir(c.id, tmp);
    poner(c.va, 1, cola.wrapping_add(usados as u32));
    if c.silencio {
        c.leido_en_silencio += usados as u64;
    }
}

/// La clase de plazo (S2, A6): hasta 3 ms de CPU cada período de 5,3 ms. Con la mezcla en coma fija (E9), un período
/// de dos flujos cuesta unos 20 µs en QEMU (antes, en coma flotante por software, 320 de media y hasta 1 ms): sobra
/// plazo, que se deja hasta medir en placa.
const PLAZO_US: (u64, u64) = (3_000, 5_333);

/// U6: pone cada flujo en silencio o con su volumen según su sesión esté detrás o no, y lo dice si cambia.
fn seguir_sesiones(m: &mut Mezclador, clientes: &mut BTreeMap<u64, Cliente>, personas: &BTreeMap<u64, u32>, detras: &[u32]) {
    for (mc, c) in clientes.iter_mut() {
        let silencio = personas.get(mc).is_some_and(|u| detras.contains(u));
        if silencio == c.silencio || c.cerrando {
            continue;
        }
        c.silencio = silencio;
        if silencio {
            m.volumen(c.id, 0.0, 0.0);
            c.leido_en_silencio = 0;
            println!("  [sonido] {}: en silencio, su sesión está detrás", c.nombre);
        } else {
            m.volumen(c.id, c.volumen.0, c.volumen.1);
            println!("  [sonido] {}: suena otra vez; en silencio se leyeron {} KB de su anillo", c.nombre, c.leido_en_silencio / 1024);
        }
    }
}

fn principal() -> u64 {
    // Llaves: 1 el permiso de la clase de plazo (S3) y 2 el canal "sonido".
    let canal = 2;
    if dashos::plazo(1, PLAZO_US.0, PLAZO_US.1) != 0 {
        println!("  [sonido] sin clase de plazo: va en la justa");
    }
    let mut m = Mezclador::nuevo(48_000);
    let mut nombres: BTreeMap<u64, String> = BTreeMap::new();
    // U6: de qué persona es cada programa, sus nombres (los de lo que lanza `sesiones`, solo para los mensajes) y las
    // personas con la sesión detrás.
    let mut personas: BTreeMap<u64, u32> = BTreeMap::new();
    let mut lanzados: BTreeMap<u64, String> = BTreeMap::new();
    let mut detras: Vec<u32> = Vec::new();
    let mut clientes: BTreeMap<u64, Cliente> = BTreeMap::new();
    let mut salida: Option<Salida> = None;
    // La marca del controlador que suena (el que mandó `SALIDA`): solo sus tics marcan el ritmo.
    let mut controlador: u64 = 0;
    let mut huecos: u64 = 0;
    let mut ultimo_tic = 0u64;
    // E9: lo que cuesta mezclar un período (µs): suma, cuántos y el peor. Se dice al cerrar cada flujo.
    let (mut mezcla_us, mut periodos, mut peor_us) = (0u64, 0u64, 0u64);
    let mut periodo: Vec<i16> = Vec::new();
    let mut tmp: Vec<u8> = Vec::new();
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 8];
    loop {
        let (n, k, marca) = match dashos::servicio::recibir(canal, &mut buf, &mut ll) {
            dashos::servicio::Recibido::Mensaje { n, k, marca } => (n, k, marca),
            dashos::servicio::Recibido::Cerrado => return 0,
            dashos::servicio::Recibido::Nada => continue,
        };
        let llaves: Vec<u32> = ll[..k].to_vec();
        let cerrar_llaves = |ll: &[u32]| ll.iter().for_each(|&l| { dashos::cerrar(l); });
        if marca == 0 {
            use dashos::protocolo::sesiones as s;
            let w = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
            if n == 24 {
                let mm = u64::from_le_bytes(buf[..8].try_into().unwrap());
                let fin = buf[8..24].iter().position(|&b| b == 0).unwrap_or(16);
                nombres.insert(mm, String::from(core::str::from_utf8(&buf[8..8 + fin]).unwrap_or("?")));
            } else if n == s::LARGO_AVISO_PERSONA && w(0) == s::AVISO_PERSONA {
                // U6: de quién es un programa (de `sesiones`; solo él e `inicio` escriben sin marca).
                let (uid, mm) = (w(4), u64::from_le_bytes(buf[8..16].try_into().unwrap()));
                let fin = buf[16..32].iter().position(|&b| b == 0).unwrap_or(16);
                if uid >= s::PRIMERA_PERSONA {
                    personas.insert(mm, uid);
                } else {
                    personas.remove(&mm);
                }
                if uid == 0 {
                    lanzados.remove(&mm);
                } else if !nombres.contains_key(&mm) {
                    lanzados.insert(mm, String::from(core::str::from_utf8(&buf[16..16 + fin]).unwrap_or("?")));
                }
                seguir_sesiones(&mut m, &mut clientes, &personas, &detras);
            } else if n == s::LARGO_AVISO_DETRAS && w(0) == s::AVISO_DETRAS {
                let cuantas = (w(4) as usize).min(s::MAX_DETRAS);
                detras = (0..cuantas).map(|i| w(8 + 4 * i)).filter(|&u| u >= s::PRIMERA_PERSONA).collect();
                seguir_sesiones(&mut m, &mut clientes, &personas, &detras);
            }
            cerrar_llaves(&llaves);
            continue;
        }
        if n < 4 {
            cerrar_llaves(&llaves);
            continue;
        }
        let w = |i: usize| u32::from_le_bytes(buf[4 * i..4 * i + 4].try_into().unwrap());
        // Los poderes (el volumen general, ser el controlador), solo por el nombre que da `inicio`.
        let nombre = nombres.get(&marca).cloned().unwrap_or_else(|| String::from("?"));
        let para_mensajes = nombres.get(&marca).or_else(|| lanzados.get(&marca)).cloned().unwrap_or_else(|| String::from("?"));
        match w(0) {
            SALIDA if n == 16 && k == 1 && CONTROLADORES.contains(&nombre.as_str()) && salida.is_none() => {
                let (ranuras, cuadros, ritmo) = (w(1), w(2), w(3));
                let cabe = (DATOS as i64 + ranuras as i64 * cuadros as i64 * 4) <= dashos::memoria_tamano(llaves[0]);
                if ritmo != 48_000 || !(1..=64).contains(&ranuras) || !(16..=4096).contains(&cuadros) || !cabe || dashos::memoria_proyectar(llaves[0], VA_SALIDA, true) != 0 {
                    println!("  [sonido] la salida de {} no vale", nombre);
                    cerrar_llaves(&llaves);
                    continue;
                }
                periodo.resize(2 * cuadros as usize, 0);
                salida = Some(Salida { ranuras, cuadros });
                controlador = marca;
                println!("  [sonido] salida por {}: {} ranuras de {} cuadros a 48 kHz", nombre, ranuras, cuadros);
            }
            // Solo el controlador marca el ritmo de la mezcla (S8).
            TIC if marca == controlador && marca != 0 => {
                cerrar_llaves(&llaves);
                let Some(s) = &salida else { continue };
                // Medida de A6: tics separados más de 15 ms, o un tic que tarda más de 3 ms en mezclar.
                let t0 = dashos::ahora_us();
                if ultimo_tic != 0 && t0 - ultimo_tic > 15_000 {
                    println!("  [sonido] {} ms sin tics", (t0 - ultimo_tic) / 1000);
                }
                ultimo_tic = t0;
                loop {
                    let (escritos, leidos) = (palabra(VA_SALIDA, 0), palabra(VA_SALIDA, 1));
                    if escritos.wrapping_sub(leidos) >= ADELANTE.min(s.ranuras) {
                        break;
                    }
                    for c in clientes.values_mut() {
                        recoger(&mut m, c, &mut tmp);
                    }
                    let tm = dashos::ahora_us();
                    m.mezclar_i16(&mut periodo);
                    let dm = dashos::ahora_us() - tm;
                    if m.flujos() > 0 {
                        mezcla_us += dm;
                        periodos += 1;
                        peor_us = peor_us.max(dm);
                    }
                    let base = VA_SALIDA + DATOS + (escritos % s.ranuras) as usize * s.cuadros as usize * 4;
                    for (i, v) in periodo.iter().enumerate() {
                        unsafe { write_volatile((base + 2 * i) as *mut i16, *v) };
                    }
                    fence(Ordering::SeqCst);
                    poner(VA_SALIDA, 0, escritos.wrapping_add(1));
                }
                let t1 = dashos::ahora_us();
                if t1 - t0 > 3_000 {
                    println!("  [sonido] un tic tardó {} us en mezclar", t1 - t0);
                }
                // Despertar a quien tenga el anillo por debajo de la mitad; quitar los flujos acabados.
                let mut fuera = Vec::new();
                for (mc, c) in clientes.iter_mut() {
                    let disp = disponibles(c);
                    let cabeza = palabra(c.va, 0);
                    if cabeza != c.ultima {
                        c.ultima = cabeza;
                        c.avisado = false;
                    }
                    if c.cerrando {
                        if disp == 0 && m.pendientes(c.id) == Some(0) {
                            m.cerrar(c.id);
                        }
                        if m.pendientes(c.id).is_none() {
                            fuera.push(*mc);
                        }
                    } else if !c.avisado && disp < c.tam / 2 {
                        if dashos::canal_escribir(c.aviso, &[1, 0, 0, 0], &[]) == E_CERRADO {
                            println!("  [sonido] {} ya no está: su flujo sale con fundido", c.nombre);
                            c.faltas = m.faltas(c.id).unwrap_or(0);
                            m.cerrar(c.id);
                            c.cerrando = true;
                        }
                        c.avisado = true;
                    }
                }
                for mc in fuera {
                    if let Some(c) = clientes.remove(&mc) {
                        println!("  [sonido] {}: flujo cerrado; {} faltas, la salida lleva {} vacíos", c.nombre, c.faltas, palabra(VA_SALIDA, 4));
                        if periodos > 0 {
                            println!("  [sonido] mezclar: {} períodos con sonido, media {} us, el peor {} us", periodos, mezcla_us / periodos, peor_us);
                        }
                        dashos::memoria_quitar(c.va);
                        dashos::cerrar(c.memoria);
                        dashos::cerrar(c.aviso);
                        huecos &= !(1 << ((c.va - VA_ANILLOS) / HUECO));
                    }
                }
            }
            ABRIR if n == 16 && k == 2 && !clientes.contains_key(&marca) => {
                let f = Muestra::desde(w(1)).map(|muestra| Formato { muestra, canales: w(2) as u8, frecuencia: w(3) });
                let hueco = (0..64).find(|h| huecos & (1 << h) == 0);
                let va = hueco.map(|h| VA_ANILLOS + h * HUECO).unwrap_or(0);
                let tamano = dashos::memoria_tamano(llaves[0]);
                let ok = f.is_some_and(|f| f.valido() && w(2) <= 2)
                    && hueco.is_some()
                    && tamano > DATOS as i64
                    && tamano as usize <= HUECO
                    && dashos::memoria_proyectar(llaves[0], va, true) == 0;
                let tam = if ok { palabra(va, 2) } else { 0 };
                if !ok || !tam.is_power_of_two() || tam as i64 + DATOS as i64 > tamano {
                    println!("  [sonido] {} pide un flujo que no vale", para_mensajes);
                    if ok {
                        dashos::memoria_quitar(va);
                    }
                    cerrar_llaves(&llaves);
                    continue;
                }
                let f = f.unwrap();
                let id = m.abrir(f).unwrap();
                huecos |= 1 << hueco.unwrap();
                println!("  [sonido] {} abre un flujo: {:?}, {} canales, {} Hz", para_mensajes, f.muestra, f.canales, f.frecuencia);
                clientes.insert(marca, Cliente {
                    nombre: para_mensajes,
                    volumen: (1.0, 1.0),
                    silencio: false,
                    leido_en_silencio: 0,
                    id,
                    va,
                    tam,
                    aviso: llaves[1],
                    memoria: llaves[0],
                    avisado: false,
                    ultima: 0,
                    cerrando: false,
                    faltas: 0,
                });
                // U6: si su sesión está detrás, en silencio desde el principio.
                seguir_sesiones(&mut m, &mut clientes, &personas, &detras);
            }
            VOLUMEN if n == 12 => {
                cerrar_llaves(&llaves);
                if let Some(c) = clientes.get_mut(&marca) {
                    c.volumen = (f32::from_bits(w(1)), f32::from_bits(w(2)));
                    // U6: en silencio, se guarda para cuando vuelva delante.
                    if !c.silencio {
                        m.volumen(c.id, c.volumen.0, c.volumen.1);
                    }
                }
            }
            GENERAL if n == 8 => {
                cerrar_llaves(&llaves);
                if PONEN_GENERAL.contains(&nombre.as_str()) {
                    let v = f32::from_bits(w(1));
                    m.volumen_general(v);
                    println!("  [sonido] volumen general: {} % (lo pide {})", (v.clamp(0.0, 4.0) * 100.0 + 0.5) as u32, nombre);
                } else {
                    println!("  [sonido] {} pide el volumen general: no le toca", nombre);
                }
            }
            CERRAR => {
                cerrar_llaves(&llaves);
                // Las faltas, de antes de cerrar: al acabarse el anillo, el último período siempre se queda corto.
                if let Some(c) = clientes.get_mut(&marca) {
                    c.faltas = m.faltas(c.id).unwrap_or(0);
                    c.cerrando = true;
                }
            }
            _ => cerrar_llaves(&llaves),
        }
    }
}
