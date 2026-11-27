//! El orquestador del banco de medidas de la multitarea (M0, docs/MULTITAREA_LOOP.md). Lo lanza `inicio` con la
//! opción `medir` del kernel (`make medir`). Su llave 1 es el paquete con los ELF de los hijos (Rust, C y Dash),
//! hecho por `tools/medir.py paquete`: "MEDP", cuántos, y por cada uno 16 bytes de nombre, desde dónde y cuánto.
//!
//! Corre las pruebas una detrás de otra y saca una línea `[medir] prueba clave=valor ...` por resultado, que lee
//! `tools/medir.py`. Los hijos hablan por texto (ver `medir_hijo.rs`).

#![no_std]
#![no_main]

#[path = "../medir_comun.rs"]
#[macro_use]
mod medir_comun;

use dashos::{
    LEER, TRANSFERIR, canal_crear, canal_escribir, canal_leer, cerrar, llave_duplicar, memoria_crear, memoria_proyectar, memoria_quitar, println,
    proceso_crear, proceso_esperar,
};
use medir_comun::*;

dashos::programa!(principal);

const PAQUETE: usize = 0x40_0000_0000;
const COPIA: usize = 0x50_0000_0000;

struct Paquete {
    base: &'static [u8],
}

impl Paquete {
    fn elf(&self, nombre: &str) -> Option<&'static [u8]> {
        let u32_en = |i: usize| u32::from_le_bytes(self.base[i..i + 4].try_into().unwrap()) as usize;
        if &self.base[..4] != b"MEDP" {
            return None;
        }
        for i in 0..u32_en(4) {
            let e = 8 + 24 * i;
            let n = &self.base[e..e + 16];
            let largo = n.iter().position(|&c| c == 0).unwrap_or(16);
            if &n[..largo] == nombre.as_bytes() {
                let (desde, cuanto) = (u32_en(e + 16), u32_en(e + 20));
                return Some(&self.base[desde..desde + cuanto]);
            }
        }
        None
    }
}

struct Hijo {
    proceso: u32,
    control: u32,
}

impl Hijo {
    fn mandar(&self, orden: &Texto) {
        canal_escribir(self.control, orden.bytes(), &[]);
    }

    fn respuesta(&self) -> Texto {
        let mut t = Texto::nuevo();
        let (n, _) = canal_leer(self.control, &mut t.b, &mut []);
        t.n = n.max(0) as usize;
        t
    }

    fn pedir(&self, orden: &Texto) -> Texto {
        self.mandar(orden);
        self.respuesta()
    }

    fn nucleo(&self) -> u64 {
        numeros::<2>(self.pedir(&texto!("nucleo")).bytes())[1]
    }

    fn acabar(self) {
        self.mandar(&texto!("fin"));
        proceso_esperar(self.proceso);
        cerrar(self.control);
        cerrar(self.proceso);
    }
}

/// Lanza el hijo `nombre` con el canal de control como llave 1 y `otro` como llave 2.
fn lanzar(p: &Paquete, nombre: &str, otro: Option<u32>) -> Option<Hijo> {
    let elf = p.elf(nombre)?;
    let (a, b) = canal_crear();
    let m = memoria_crear(elf.len());
    if (m as i32) <= 0 || memoria_proyectar(m, COPIA, true) != 0 {
        println!("  [medir] {}: sin memoria", nombre);
        return None;
    }
    unsafe { core::ptr::copy_nonoverlapping(elf.as_ptr(), COPIA as *mut u8, elf.len()) };
    memoria_quitar(COPIA);
    let mut llaves = [b, 0];
    let n = match otro {
        Some(o) => {
            llaves[1] = o;
            2
        }
        None => 1,
    };
    let proceso = proceso_crear(m, elf.len(), &llaves[..n]);
    cerrar(m);
    if proceso <= 0 {
        println!("  [medir] {}: no se lanza ({})", nombre, proceso);
        return None;
    }
    Some(Hijo { proceso: proceso as u32, control: a })
}

fn hijo_de(lenguaje: &str) -> &'static str {
    match lenguaje {
        "c" => "hijo-c",
        "dash" => "hijo-dash",
        _ => "hijo-rust",
    }
}

/// Lanza `nombre` hasta que uno caiga en el núcleo `en` (y se fija ahí); los que no, quedan en `relleno` sin gastar
/// CPU hasta el final de la prueba. Sin `en`, el primero, sin fijar.
fn colocar(p: &Paquete, nombre: &str, en: Option<u64>, relleno: &mut [Option<Hijo>; 24]) -> Option<Hijo> {
    for _ in 0..24 {
        let h = lanzar(p, nombre, None)?;
        let Some(n) = en else { return Some(h) };
        if h.nucleo() == n {
            return Some(h);
        }
        match relleno.iter_mut().find(|r| r.is_none()) {
            Some(r) => *r = Some(h),
            None => h.acabar(),
        }
    }
    None
}

/// Dónde ve el orquestador la bandera que acaba las cargas.
const BANDERA: usize = 0x58_0000_0000;

/// P1: idas y vueltas entre dos hijos, en el mismo núcleo (`juntos`), en dos distintos o donde los ponga el
/// planificador (`None`), con `carga` hijos calculando a la vez.
///
/// R7 (B1): todo en su sitio, siempre el mismo respecto al orquestador (fijado en su núcleo `o` al empezar): el eco
/// en el núcleo o + 1 y el ping en ese (juntos) o en o + 2; cada carga fijada en su núcleo (0, 1, 2, 3), calculando
/// hasta que acaba la medida. El núcleo del orquestador no se usa para ellos: cuenta como ocupado y el kernel no
/// pondría ahí un proceso nuevo. El hijo
/// de Dash no sabe en qué núcleo está: con él, donde caiga (nucleos 9).
fn ida_vuelta(p: &Paquete, lenguaje: &str, rondas: u64, juntos: Option<bool>, carga: usize) {
    let mut relleno: [Option<Hijo>; 24] = Default::default();
    let mut cargas: [Option<Hijo>; 4] = Default::default();
    let bandera = if carga > 0 { memoria_crear(4096) } else { 0 };
    let con_bandera = (bandera as i32) > 0 && memoria_proyectar(bandera, BANDERA, true) == 0;
    if con_bandera {
        unsafe { core::ptr::write_volatile(BANDERA as *mut u8, 0) };
        for (i, c) in cargas.iter_mut().take(carga).enumerate() {
            *c = colocar(p, "hijo-rust", Some(i as u64), &mut relleno);
            if let Some(h) = c {
                let k = llave_duplicar(bandera, LEER | TRANSFERIR, 0);
                canal_escribir(h.control, b"carga", &[k as u32]);
            }
        }
    }
    let sabe = lenguaje != "dash" && juntos.is_some();
    let (x, y) = canal_crear();
    let (eco, ping) = if sabe {
        let o = nucleo();
        let (n_eco, n_ping) = ((o + 1) % 4, if juntos == Some(true) { (o + 1) % 4 } else { (o + 2) % 4 });
        let eco = colocar(p, hijo_de(lenguaje), Some(n_eco), &mut relleno);
        let ping = colocar(p, hijo_de(lenguaje), Some(n_ping), &mut relleno);
        (eco, ping)
    } else {
        (lanzar(p, hijo_de(lenguaje), Some(x)), lanzar(p, hijo_de(lenguaje), None))
    };
    if let (Some(eco), Some(ping)) = (&eco, &ping) {
        // El canal con el eco va con la orden (o, sin colocar, nació con él).
        if sabe {
            canal_escribir(eco.control, b"eco", &[x]);
        } else {
            eco.mandar(&texto!("eco"));
        }
        let orden = texto!("ping {}", rondas);
        let espera = || [8u64, 9, 10, 11].map(|q| dashos::llamada(29, [q, 0, 0, 0, 0]).0);
        let e0 = espera();
        canal_escribir(ping.control, orden.bytes(), &[y]);
        let r = ping.respuesta();
        let e1 = espera();
        let [desp, tarde, us_desp, no_adelanta] = [0, 1, 2, 3].map(|i| e1[i].wrapping_sub(e0[i]));
        let [_, n, us, n_ping, p50, p99, max] = numeros::<7>(r.bytes());
        let _ = eco.respuesta();
        println!(
            "[medir] ida-vuelta lenguaje={} juntos={} carga={} rondas={} ns={} p50_ns={} p99_ns={} max_ns={} nucleos={}-{} despiertan={} tarde={} espera_us={} no_adelanta={}",
            lenguaje,
            juntos.map_or(2, |j| j as u8),
            carga,
            n,
            if n > 0 { us * 1000 / n } else { 0 },
            p50,
            p99,
            max,
            if sabe { (nucleo() + 1) % 4 } else { 9 },
            if sabe { n_ping } else { 9 },
            desp,
            tarde,
            if desp > 0 { us_desp / desp } else { 0 },
            no_adelanta
        );
    } else {
        println!("  [medir] no se pudo colocar la ida y vuelta de {} (juntos {:?}, carga {})", lenguaje, juntos, carga);
        cerrar(x);
        cerrar(y);
    }
    for h in [eco, ping].into_iter().flatten() {
        h.acabar();
    }
    if con_bandera {
        unsafe { core::ptr::write_volatile(BANDERA as *mut u8, 1) };
    }
    for c in cargas.into_iter().flatten() {
        let _ = c.respuesta();
        c.acabar();
    }
    if con_bandera {
        memoria_quitar(BANDERA);
    }
    if (bandera as i32) > 0 {
        cerrar(bandera);
    }
    for h in relleno.into_iter().flatten() {
        h.acabar();
    }
}

/// P2: `n` hijos calculan `ms` a la vez.
fn reparto(p: &Paquete, lenguaje: &str, n: usize, ms: u64) {
    let mut hijos: [Option<Hijo>; 8] = Default::default();
    for h in hijos.iter_mut().take(n) {
        *h = lanzar(p, hijo_de(lenguaje), None);
    }
    let antes = contadores();
    let robos = contador(4, 0);
    for h in hijos.iter().flatten() {
        h.mandar(&texto!("calc {}", ms));
    }
    let (mut total, mut min, mut max, mut hueco) = (0u64, u64::MAX, 0u64, 0u64);
    let mut nucleos = Texto::nuevo();
    for h in hijos.iter().flatten() {
        let [_, u, hm, _, n0, n1] = numeros::<6>(h.respuesta().bytes());
        total += u;
        min = min.min(u);
        max = max.max(u);
        hueco = hueco.max(hm);
        let coma = if nucleos.n > 0 { "," } else { "" };
        let _ = core::fmt::Write::write_fmt(&mut nucleos, format_args!("{}{}", coma, n0));
        if n1 != n0 {
            let _ = core::fmt::Write::write_fmt(&mut nucleos, format_args!(">{}", n1));
        }
    }
    let despues = contadores();
    println!(
        "[medir] reparto lenguaje={} n={} ms={} total={} min={} max={} justo={} hueco_max_us={} cambios={} robos={} nucleos={}",
        lenguaje,
        n,
        ms,
        total,
        min,
        max,
        if max > 0 { min * 1000 / max } else { 0 },
        hueco,
        despues.1 - antes.1,
        contador(4, 0) - robos,
        core::str::from_utf8(nucleos.bytes()).unwrap_or("?")
    );
    for h in hijos.into_iter().flatten() {
        h.acabar();
    }
}

/// Tics y cambios de hilo sumados de los cuatro núcleos.
fn contadores() -> (u64, u64) {
    (0..4).fold((0, 0), |(t, c), n| (t + contador(1, n), c + contador(2, n)))
}

/// P3: esperas cortas con un temporizador, sin carga y con cuatro hijos calculando.
fn temporizador(p: &Paquete, us: u64, veces: u64, carga: usize) {
    let mut cargas: [Option<Hijo>; 4] = Default::default();
    for c in cargas.iter_mut().take(carga) {
        *c = lanzar(p, "hijo-rust", None);
    }
    let Some(h) = lanzar(p, "hijo-rust", None) else { return };
    for c in cargas.iter().flatten() {
        c.mandar(&texto!("calc {}", veces * us / 1000 * 3 + 2000));
    }
    let r = h.pedir(&texto!("temp {} {}", us, veces));
    let [_, p50, p99, max] = numeros::<4>(r.bytes());
    println!("[medir] temporizador us={} veces={} carga={} p50_us={} p99_us={} max_us={}", us, veces, carga, p50, p99, max);
    h.acabar();
    for c in cargas.into_iter().flatten() {
        let _ = c.respuesta();
        c.acabar();
    }
}

/// P4: un hijo duerme `dormir_ms` mientras otro de su núcleo calcula, y luego calcula él: ¿cuánto deja sin CPU al
/// otro? Si no caen en el mismo núcleo, se lanza relleno.
fn dormilon(p: &Paquete, dormir_ms: u64) {
    let Some(v) = lanzar(p, "hijo-rust", None) else { return };
    let nv = v.nucleo();
    let mut relleno: [Option<Hijo>; 8] = Default::default();
    let mut d = None;
    for r in relleno.iter_mut() {
        let Some(h) = lanzar(p, "hijo-rust", None) else { break };
        if h.nucleo() == nv {
            d = Some(h);
            break;
        }
        *r = Some(h);
    }
    let Some(d) = d else {
        println!("  [medir] dormilón: no se pudo colocar");
        v.acabar();
        for h in relleno.into_iter().flatten() {
            h.acabar();
        }
        return;
    };
    let calc = dormir_ms + 2000;
    v.mandar(&texto!("calc {}", calc));
    let _ = d.pedir(&texto!("dormir {}", dormir_ms));
    let rd = d.pedir(&texto!("calc 1000"));
    let rv = v.respuesta();
    let [_, _, hueco, huecos, ..] = numeros::<6>(rv.bytes());
    let [_, ud, ..] = numeros::<6>(rd.bytes());
    println!("[medir] dormilon dormir_ms={} hueco_max_us={} huecos_us={} unidades_dormilon={} nucleo={}", dormir_ms, hueco, huecos, ud, nv);
    v.acabar();
    d.acabar();
    for h in relleno.into_iter().flatten() {
        h.acabar();
    }
}

/// P6: tics por segundo de cada núcleo con el sistema parado (el orquestador duerme).
fn ociosos() {
    let antes: [u64; 4] = core::array::from_fn(|n| contador(1, n as u64));
    let ocio0: [u64; 4] = core::array::from_fn(|n| contador(3, n as u64));
    let t0 = cuentas();
    dashos::dormir_us(1_000_000);
    let us = a_us(cuentas() - t0).max(1);
    let mut t = Texto::nuevo();
    for n in 0..4 {
        let tics = (contador(1, n as u64) - antes[n]) * 1_000_000 / us;
        let ocio = (contador(3, n as u64) - ocio0[n]) * 1000 / us;
        let _ = core::fmt::Write::write_fmt(&mut t, format_args!(" tics{}={} ocio{}={}", n, tics, n, ocio));
    }
    println!("[medir] ociosos{}", core::str::from_utf8(t.bytes()).unwrap_or(""));
}

/// P7 (R): la memoria. Ocupada nada más arrancar (páginas de RAM menos las libres), el kernel en sí y lo que cuesta
/// cada proceso de más: 20 hijos que esperan una orden sin hacer nada, y la diferencia de páginas libres entre 20.
fn memoria(p: &Paquete) {
    let total = contador(5, 0);
    let libres0 = contador(6, 0);
    let kernel = contador(7, 0);
    let mut hijos: [Option<Hijo>; 20] = Default::default();
    for h in hijos.iter_mut() {
        *h = lanzar(p, "hijo-rust", None);
    }
    dashos::dormir_us(200_000);
    let libres1 = contador(6, 0);
    let vivos = hijos.iter().filter(|h| h.is_some()).count() as u64;
    for h in hijos.into_iter().flatten() {
        h.acabar();
    }
    println!(
        "[medir] memoria usada_kb={} kernel_kb={} por_proceso_kb={} total_kb={} procesos={}",
        (total - libres0) * 4,
        kernel / 1024,
        if vivos > 0 { libres0.saturating_sub(libres1) * 4 / vivos } else { 0 },
        total * 4,
        vivos
    );
}

fn principal() -> u64 {
    if memoria_proyectar(1, PAQUETE, false) != 0 {
        println!("  [medir] sin paquete");
        return 1;
    }
    let tamano = dashos::memoria_tamano(1).max(0) as usize;
    let p = Paquete { base: unsafe { core::slice::from_raw_parts(PAQUETE as *const u8, tamano) } };
    // Fijo en su núcleo (R7): la ida y vuelta coloca a los suyos respecto a él.
    let o = fijar();
    println!("[medir] empieza (orquestador en el núcleo {})", o);
    memoria(&p);
    ociosos();
    for lenguaje in ["rust", "c"] {
        ida_vuelta(&p, lenguaje, 20_000, Some(false), 0);
        ida_vuelta(&p, lenguaje, 20_000, Some(true), 0);
    }
    ida_vuelta(&p, "dash", 20_000, Some(false), 0);
    // Con carga: los dos donde los ponga el planificador (juntos=2), y fijos juntos y separados (M10); las cuatro
    // cargas, siempre una fijada en cada núcleo (R7).
    ida_vuelta(&p, "rust", 5_000, None, 4);
    ida_vuelta(&p, "rust", 5_000, Some(true), 4);
    ida_vuelta(&p, "rust", 5_000, Some(false), 4);
    for n in [1, 2, 3, 4, 5, 6, 8] {
        reparto(&p, "rust", n, 2000);
    }
    reparto(&p, "c", 4, 2000);
    reparto(&p, "dash", 4, 2000);
    // Una tanda para calentar (la primera de cada arranque sale peor) y luego las que cuentan.
    temporizador(&p, 1000, 50, 0);
    temporizador(&p, 1000, 300, 0);
    temporizador(&p, 1000, 300, 4);
    dormilon(&p, 2000);
    println!("[medir] fin");
    0
}
