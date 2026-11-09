//! El servicio de sonido (S19, A4): mezcla los flujos de los programas y entrega períodos al controlador.
//!
//! Por el canal "sonido" le llega todo (un proceso solo espera en un canal): los mensajes de `inicio` (sin marca: qué
//! marca es de quién), los del controlador y los de los programas, cada uno con su marca. El protocolo, en
//! `docs/referencia/servicios.md`:
//!
//! - El controlador (solo el que `inicio` llama "virtio-sonido") manda `SALIDA` con una memoria compartida: cabecera
//!   (escritos, leídos, ranuras, cuadros por período) y las ranuras desde el byte 64, cada una un período en estéreo de
//!   16 bits. Después, un `TIC` cada vez que el aparato devuelve un búfer: el reloj del hardware manda (S19). Con cada
//!   tic, el servicio mezcla hasta tener `ADELANTE` períodos listos.
//! - Un programa manda `ABRIR` (formato) con la memoria de su anillo y el extremo de escritura de un canal suyo
//!   (`usuario/src/sonido_cliente.rs`); `VOLUMEN` y `CERRAR`. Un flujo por programa.
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
}

/// La clase de plazo (S2, A6): hasta 3 ms de CPU cada período de 5,3 ms. Mezclar va en coma flotante por software, y
/// en QEMU un período de dos flujos cuesta 1 a 2 ms.
const PLAZO_US: (u64, u64) = (3_000, 5_333);

fn principal() -> u64 {
    let canal = 1;
    if dashos::plazo(PLAZO_US.0, PLAZO_US.1) != 0 {
        println!("  [sonido] sin clase de plazo: va en la justa");
    }
    let mut m = Mezclador::nuevo(48_000);
    let mut nombres: BTreeMap<u64, String> = BTreeMap::new();
    let mut clientes: BTreeMap<u64, Cliente> = BTreeMap::new();
    let mut salida: Option<Salida> = None;
    let mut huecos: u64 = 0;
    let mut ultimo_tic = 0u64;
    let mut periodo: Vec<i16> = Vec::new();
    let mut tmp: Vec<u8> = Vec::new();
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 8];
    loop {
        let (n, k, marca) = dashos::canal_leer_marca(canal, &mut buf, &mut ll);
        if n == E_CERRADO {
            return 0;
        }
        if n < 0 {
            continue;
        }
        let n = n as usize;
        let llaves: Vec<u32> = ll[..k].to_vec();
        let cerrar_llaves = |ll: &[u32]| ll.iter().for_each(|&l| { dashos::cerrar(l); });
        if marca == 0 {
            if n == 24 {
                let mm = u64::from_le_bytes(buf[..8].try_into().unwrap());
                let fin = buf[8..24].iter().position(|&b| b == 0).unwrap_or(16);
                nombres.insert(mm, String::from(core::str::from_utf8(&buf[8..8 + fin]).unwrap_or("?")));
            }
            cerrar_llaves(&llaves);
            continue;
        }
        if n < 4 {
            cerrar_llaves(&llaves);
            continue;
        }
        let w = |i: usize| u32::from_le_bytes(buf[4 * i..4 * i + 4].try_into().unwrap());
        let nombre = nombres.get(&marca).cloned().unwrap_or_else(|| String::from("?"));
        match w(0) {
            SALIDA if n == 16 && k == 1 && nombre == "virtio-sonido" && salida.is_none() => {
                let (ranuras, cuadros, ritmo) = (w(1), w(2), w(3));
                let cabe = (DATOS as i64 + ranuras as i64 * cuadros as i64 * 4) <= dashos::memoria_tamano(llaves[0]);
                if ritmo != 48_000 || !(1..=64).contains(&ranuras) || !(16..=4096).contains(&cuadros) || !cabe || dashos::memoria_proyectar(llaves[0], VA_SALIDA, true) != 0 {
                    println!("  [sonido] la salida de {} no vale", nombre);
                    cerrar_llaves(&llaves);
                    continue;
                }
                periodo.resize(2 * cuadros as usize, 0);
                salida = Some(Salida { ranuras, cuadros });
                println!("  [sonido] salida por {}: {} ranuras de {} cuadros a 48 kHz", nombre, ranuras, cuadros);
            }
            TIC => {
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
                    m.mezclar_i16(&mut periodo);
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
                    println!("  [sonido] {} pide un flujo que no vale", nombre);
                    if ok {
                        dashos::memoria_quitar(va);
                    }
                    cerrar_llaves(&llaves);
                    continue;
                }
                let f = f.unwrap();
                let id = m.abrir(f).unwrap();
                huecos |= 1 << hueco.unwrap();
                println!("  [sonido] {} abre un flujo: {:?}, {} canales, {} Hz", nombre, f.muestra, f.canales, f.frecuencia);
                clientes.insert(marca, Cliente { nombre, id, va, tam, aviso: llaves[1], memoria: llaves[0], avisado: false, ultima: 0, cerrando: false, faltas: 0 });
            }
            VOLUMEN if n == 12 => {
                if let Some(c) = clientes.get(&marca) {
                    m.volumen(c.id, f32::from_bits(w(1)), f32::from_bits(w(2)));
                }
            }
            CERRAR => {
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
