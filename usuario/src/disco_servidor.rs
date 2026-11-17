//! El servidor del canal "disco" (U6f, A13 a A16) para los controladores de discos que no son el NVMe: hoy, el AHCI
//! (X5c). El protocolo está en `docs/referencia/servicios.md` y es el de `bin/nvme.rs`, que sigue con su copia para no
//! mover lo de ARM; aquí va separado del controlador, que solo sabe mandar órdenes al disco (`Controlador`).
//!
//! Lo que añade al protocolo del NVMe: un controlador puede llevar varios discos (los puertos del AHCI). Al abrir (orden
//! 0), los bytes 8-15 dicen qué disco (0 el primero); abrir otro cambia el disco del cliente y la ventana es la misma.
//! El NVMe solo tiene el 0 y no mira ese campo. Cada disco tiene su tabla de particiones, y el tramo de cada cliente
//! (por su nombre) es el de ese disco.

use crate::println;
use core::sync::atomic::{AtomicU32, Ordering::*};

/// Órdenes del disco en vuelo a la vez, como mucho (A15); cada una tiene su sitio (0 a `EN_VUELO` - 1).
pub const EN_VUELO: usize = 8;
/// La ventana de cada cliente: 128 KB de memoria DMA (dos mitades de 64 KB para el servicio de archivos, A14).
pub const VENTANA: usize = 128 * 1024;
/// Discos de un controlador, como mucho.
pub const MAX_DISCOS: usize = 8;

/// Una orden para el disco. `bus` es la dirección de bus de los datos (en la ventana del cliente), seguidos.
#[derive(Clone, Copy)]
pub enum Orden {
    Leer { primero: u64, cuantos: u32, bus: u64 },
    Escribir { primero: u64, cuantos: u32, bus: u64 },
    Vaciar,
    Descartar { primero: u64, cuantos: u64 },
}

/// Lo que el servidor necesita del controlador.
pub trait Controlador {
    /// Cuántos discos lleva (los números van de 0 a este menos uno).
    fn discos(&self) -> usize;
    fn sectores(&self, d: usize) -> u64;
    fn tam_sector(&self, d: usize) -> usize;
    /// Cuánto se espera a una orden antes de darla por perdida.
    fn plazo_ms(&self) -> u64;
    /// Si el disco sabe descartar (TRIM); si no, descartar contesta bien sin hacer nada.
    fn descarta(&self, d: usize) -> bool;
    /// Lo más que lleva una lectura o una escritura, en bytes.
    fn max_bytes(&self, d: usize) -> usize;
    /// Manda `o` al disco `d` sin esperar: su identificador; nada si no hay sitio. `sitio` es suyo mientras esté en
    /// vuelo (para lo que necesite en memoria, como la lista de tramos de un descarte).
    fn enviar(&mut self, d: usize, o: Orden, sitio: usize) -> Option<u16>;
    /// La siguiente orden terminada del disco `d`, esperando como mucho `ms` (con 0, solo se mira): su identificador
    /// y si fue bien. Nada si no llega a tiempo.
    fn terminada(&mut self, d: usize, ms: u64) -> Option<(u16, bool)>;
    /// Lee el sector `s` del disco `d` en `b` (un sector), esperando (al arrancar: la tabla de particiones).
    fn leer_sector(&mut self, d: usize, s: u64, b: &mut [u8]) -> bool;
}

const E_CERRADO: i64 = -4;
const PEDIR_ABRIR: u32 = 0;
const PEDIR_LEER: u32 = 1;
const PEDIR_ESCRIBIR: u32 = 2;
const PEDIR_ANILLO: u32 = 3;
const PEDIR_VACIAR: u32 = 5;
const PEDIR_DESCARTAR: u32 = 6;
const PEDIR_LEER_SIN_ESPERA: u32 = 7;
const MAL_FORMADO: i32 = -1;
const FUERA_DEL_DISCO: i32 = -2;
const FUERA_DE_LA_VENTANA: i32 = -3;
const ERROR_DEL_DISCO: i32 = -4;
const SIN_SITIO: i32 = -5;
const SIN_PERMISO: i32 = -6;
const MAX_CLIENTES: usize = 5;

/// Los programas con tramo, por el nombre que les da `inicio` (A13), como en el NVMe, y `disco-sata`, la prueba del
/// AHCI (X5c), que ve lo mismo que `disco`: el primer MB sin llegar a la primera partición (con GPT, nada).
/// S32: y el instalador (solo en vivo, arrancado del pendrive), con cada disco entero: lee las tablas de todos, aunque
/// no valgan, y escribe solo lo que dice su plan.
const NOMBRES: [&str; 6] = ["disco", "archivos", "comprobador", "caja-linux", "disco-sata", "instalador"];
/// El del instalador, por su número en `NOMBRES`.
const INSTALADOR: usize = 5;
/// El de solo lectura, por su número en `NOMBRES`.
const SOLO_LEER: usize = 2;
/// Los tramos de un disco, por el número en `NOMBRES` (sin el último, que es como el primero).
type Tramos = [Option<(u64, u64)>; 4];

// El anillo de cada cliente (U6l): una página compartida; ver `bin/nvme.rs` y `docs/referencia/servicios.md`.
const VA_ANILLOS: usize = 0x31_0000_0000;
const A_SQ_CABEZA: usize = 0;
const A_SQ_COLA: usize = 4;
const A_CQ_CABEZA: usize = 8;
const A_CQ_COLA: usize = 12;
const A_DUERME: usize = 16;
const A_ESPERA: usize = 20;
const A_SQ: usize = 64;
const A_CQ: usize = 2048;
const A_ENTRADAS: u32 = 32;

#[derive(Clone, Copy)]
struct Cliente {
    marca: u64,
    llave: u32,
    bus: u64,
    anillo: Option<(u32, usize, u32)>,
    /// El disco que abrió y su tramo en él, [desde, hasta).
    disco: usize,
    tramo: (u64, u64),
    solo_leer: bool,
}

#[derive(Clone, Copy)]
struct Pieza {
    orden: u32,
    primero: u64,
    cuantos: u64,
    off: usize,
    etiqueta: u32,
    id: Option<u16>,
    estado: i32,
}

fn anillo(va: usize, off: usize) -> &'static AtomicU32 {
    // La página del anillo está proyectada aquí mientras el cliente lo tiene; los campos van alineados a 4.
    unsafe { &*((va + off) as *const AtomicU32) }
}

/// Lee la tabla de particiones de cada disco y da sus tramos (A13, A14). `nombre` es el del servicio, para el registro.
fn tramos_de_los_discos<C: Controlador>(c: &mut C, nombre: &str) -> [Tramos; MAX_DISCOS] {
    use crate::particiones::Clase;
    let mut todos = [[None; 4]; MAX_DISCOS];
    for (d, t) in todos.iter_mut().enumerate().take(c.discos()) {
        let (sectores, tam) = (c.sectores(d), c.tam_sector(d));
        let mut leer = |s: u64, b: &mut [u8]| c.leer_sector(d, s, b);
        let mb = (1u64 << 20) / tam as u64;
        *t = match crate::particiones::leer_tabla(&mut leer, sectores, tam) {
            Ok(tabla) => {
                let clase = match tabla.clase {
                    Clase::Gpt => "GPT",
                    Clase::Mbr => "MBR",
                    Clase::Ninguna => "ninguna",
                };
                match tabla.dashos {
                    Some((a, b)) => println!("  [{}] disco {}: particiones: {}; DashOS, sectores {} a {}", nombre, d, clase, a, b - 1),
                    None => println!("  [{}] disco {}: particiones: {}; ninguna de DashOS", nombre, d, clase),
                }
                if let Some((a, b)) = tabla.linux {
                    println!("  [{}] disco {}: la caja Linux: sectores {} a {}", nombre, d, a, b - 1);
                }
                let crudo = (tabla.clase != Clase::Gpt).then_some((0, mb.min(tabla.primera).min(sectores)));
                [crudo, tabla.dashos, tabla.dashos, tabla.linux]
            }
            Err(e) => {
                println!("  [{}] disco {}: la tabla de particiones no vale ({:?}): no se abre a nadie", nombre, d, e);
                [None; 4]
            }
        };
    }
    todos
}

/// El número en `NOMBRES` de la marca `marca`, por el directorio de `inicio`.
fn nombre_de(nombres: &[(u64, [u8; 16])], marca: u64) -> Option<usize> {
    let (_, n) = nombres.iter().find(|e| e.0 == marca && marca != 0)?;
    let nombre = &n[..n.iter().position(|&c| c == 0).unwrap_or(16)];
    NOMBRES.iter().position(|t| t.as_bytes() == nombre).map(|i| if i == 4 { 0 } else { i })
}

/// Comprueba una lectura, escritura o descarte de `c` y la convierte en una orden del disco. Si no hay que mandar
/// nada, el estado: el del rechazo, o 0 (un descarte en un disco que no sabe).
fn preparar<C: Controlador>(ctl: &C, c: &Cliente, orden: u32, cuantos: u64, primero: u64, off: usize) -> Result<Orden, i32> {
    let tam = ctl.tam_sector(c.disco);
    if orden != PEDIR_LEER && orden != PEDIR_ESCRIBIR && orden != PEDIR_DESCARTAR {
        return Err(MAL_FORMADO);
    }
    if c.solo_leer && orden != PEDIR_LEER {
        return Err(SIN_PERMISO);
    }
    if cuantos == 0 || primero.checked_add(cuantos).is_none_or(|f| f > c.tramo.1 - c.tramo.0) {
        return Err(FUERA_DEL_DISCO);
    }
    let primero = c.tramo.0 + primero;
    if orden == PEDIR_DESCARTAR {
        return if ctl.descarta(c.disco) { Ok(Orden::Descartar { primero, cuantos }) } else { Err(0) };
    }
    let bytes = (cuantos as usize).saturating_mul(tam);
    if off % 4 != 0 || off.checked_add(bytes).is_none_or(|f| f > VENTANA) || bytes > ctl.max_bytes(c.disco) {
        return Err(FUERA_DE_LA_VENTANA);
    }
    let (cuantos, bus) = (cuantos as u32, c.bus + off as u64);
    Ok(if orden == PEDIR_LEER { Orden::Leer { primero, cuantos, bus } } else { Orden::Escribir { primero, cuantos, bus } })
}

/// Manda una orden y espera su terminada; las de órdenes anteriores que vencieron su plazo se descartan (A16).
fn orden<C: Controlador>(ctl: &mut C, d: usize, o: Orden, sin_esperar: bool, nombre: &str) -> bool {
    let Some(id) = ctl.enviar(d, o, 0) else { return false };
    loop {
        let ms = if sin_esperar { 0 } else { ctl.plazo_ms() };
        let Some((t, bien)) = ctl.terminada(d, ms) else { return false };
        if t == id {
            return bien;
        }
        println!("  [{}] terminada tardía descartada (orden {})", nombre, t);
    }
}

/// Una petición suelta (no del anillo): su estado.
fn transferir<C: Controlador>(ctl: &mut C, c: &Cliente, orden_: u32, cuantos: u64, primero: u64, off: usize, nombre: &str) -> i32 {
    let (orden_, sin_esperar) = if orden_ == PEDIR_LEER_SIN_ESPERA { (PEDIR_LEER, true) } else { (orden_, false) };
    if orden_ == PEDIR_VACIAR {
        return if c.solo_leer {
            SIN_PERMISO
        } else if orden(ctl, c.disco, Orden::Vaciar, false, nombre) {
            0
        } else {
            ERROR_DEL_DISCO
        };
    }
    match preparar(ctl, c, orden_, cuantos, primero, off) {
        Err(e) => e,
        Ok(o) => {
            if orden(ctl, c.disco, o, sin_esperar, nombre) {
                0
            } else {
                ERROR_DEL_DISCO
            }
        }
    }
}

/// Si `b`, detrás de `a` en el anillo, tiene que esperar a que `a` acabe (el disco no guarda el orden entre órdenes
/// en vuelo): el mismo sector y una escribe o descarta, o el mismo sitio de la ventana y una lee.
fn choca(a: &Pieza, b: &Pieza, tam: usize) -> bool {
    let tramo = |p: &Pieza| (p.primero, p.primero.saturating_add(p.cuantos));
    let ventana = |p: &Pieza| (p.off as u64, (p.off as u64).saturating_add(p.cuantos.saturating_mul(tam as u64)));
    let pisa = |x: (u64, u64), y: (u64, u64)| x.0 < y.1 && y.0 < x.1;
    let cambia = |p: &Pieza| p.orden != PEDIR_LEER;
    let en_ventana = |p: &Pieza| p.orden != PEDIR_DESCARTAR;
    (pisa(tramo(a), tramo(b)) && (cambia(a) || cambia(b)))
        || (en_ventana(a) && en_ventana(b) && pisa(ventana(a), ventana(b)) && (a.orden == PEDIR_LEER || b.orden == PEDIR_LEER))
}

/// Atiende el anillo de `c` (A15: lotes de hasta `EN_VUELO` órdenes a la vez, con las terminadas en el orden del
/// anillo). Devuelve cuántos envíos atendió.
fn atender_anillo<C: Controlador>(ctl: &mut C, c: &Cliente, nombre: &str) -> u32 {
    let Some((_, va, aviso)) = c.anillo else { return 0 };
    let tam = ctl.tam_sector(c.disco);
    let mut hechas = 0;
    loop {
        let (cabeza, cola) = (anillo(va, A_SQ_CABEZA).load(Relaxed), anillo(va, A_SQ_COLA).load(Acquire));
        let (cq_cabeza, cq_cola) = (anillo(va, A_CQ_CABEZA).load(Acquire), anillo(va, A_CQ_COLA).load(Relaxed));
        let sitio = A_ENTRADAS.saturating_sub(cq_cola.wrapping_sub(cq_cabeza));
        let pendientes = cola.wrapping_sub(cabeza).min(A_ENTRADAS);
        if pendientes == 0 || sitio == 0 {
            break;
        }
        let mut lote = [Pieza { orden: 0, primero: 0, cuantos: 0, off: 0, etiqueta: 0, id: None, estado: 0 }; EN_VUELO];
        let mut nl = 0;
        while nl < EN_VUELO && (nl as u32) < pendientes.min(sitio) {
            // Se copia el envío antes de mirarlo: el cliente puede cambiarlo a la vez.
            let e = va + A_SQ + 32 * (cabeza.wrapping_add(nl as u32) % A_ENTRADAS) as usize;
            let mut b = [0u8; 32];
            for (i, x) in b.iter_mut().enumerate() {
                *x = unsafe { core::ptr::read_volatile((e + i) as *const u8) };
            }
            let u32_en = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
            let mut p = Pieza {
                orden: u32_en(0),
                cuantos: u32_en(4) as u64,
                primero: u32_en(8) as u64 | (u32_en(12) as u64) << 32,
                off: u32_en(16) as usize,
                etiqueta: u32_en(20),
                id: None,
                estado: 0,
            };
            let junta = p.orden == PEDIR_LEER || p.orden == PEDIR_ESCRIBIR || p.orden == PEDIR_DESCARTAR;
            if !junta {
                // Sola, con nada en vuelo.
                if nl == 0 {
                    p.estado = transferir(ctl, c, p.orden, p.cuantos, p.primero, p.off, nombre);
                    lote[0] = p;
                    nl = 1;
                }
                break;
            }
            if lote[..nl].iter().any(|a| choca(a, &p, tam)) {
                break;
            }
            match preparar(ctl, c, p.orden, p.cuantos, p.primero, p.off) {
                Err(e) => p.estado = e,
                Ok(o) => match ctl.enviar(c.disco, o, nl) {
                    Some(id) => p.id = Some(id),
                    None => p.estado = SIN_SITIO,
                },
            }
            lote[nl] = p;
            nl += 1;
        }
        let fin = crate::ahora_us() + ctl.plazo_ms() * 1000;
        while lote[..nl].iter().any(|p| p.id.is_some()) {
            let ms = fin.saturating_sub(crate::ahora_us()).div_ceil(1000);
            match ctl.terminada(c.disco, ms.max(1)) {
                None => {
                    for p in lote[..nl].iter_mut().filter(|p| p.id.is_some()) {
                        p.id = None;
                        p.estado = ERROR_DEL_DISCO;
                    }
                }
                Some((id, bien)) => match lote[..nl].iter_mut().find(|p| p.id == Some(id)) {
                    Some(p) => {
                        p.id = None;
                        p.estado = if bien { 0 } else { ERROR_DEL_DISCO };
                    }
                    None => println!("  [{}] terminada tardía descartada (orden {})", nombre, id),
                },
            }
        }
        for (k, p) in lote[..nl].iter().enumerate() {
            let t = va + A_CQ + 16 * (cq_cola.wrapping_add(k as u32) % A_ENTRADAS) as usize;
            anillo(t, 0).store(p.etiqueta, Relaxed);
            anillo(t, 4).store(p.estado as u32, Relaxed);
        }
        anillo(va, A_CQ_COLA).store(cq_cola.wrapping_add(nl as u32), Release);
        anillo(va, A_SQ_CABEZA).store(cabeza.wrapping_add(nl as u32), Release);
        hechas += nl as u32;
    }
    core::sync::atomic::fence(SeqCst);
    if hechas > 0 && anillo(va, A_ESPERA).swap(0, SeqCst) != 0 {
        crate::canal_escribir(aviso, &[0; 4], &[]);
    }
    hechas
}

/// Sirve los discos de `ctl` por el canal de la llave `canal`, con la memoria DMA del permiso `permiso` para las
/// ventanas. `nombre` es el del servicio (el registro). No vuelve hasta que se cierra el canal.
pub fn servir<C: Controlador>(ctl: &mut C, permiso: u32, canal: u32, nombre: &str) -> u64 {
    let tramos = tramos_de_los_discos(ctl, nombre);
    let mut clientes: [Option<Cliente>; MAX_CLIENTES] = [None; MAX_CLIENTES];
    let mut nombres: [(u64, [u8; 16]); 8] = [(0, [0; 16]); 8];
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 8];
    loop {
        // "Duermo" en cada anillo y una última mirada antes de dormir (como en el NVMe).
        for c in clientes.iter().flatten() {
            if let Some((_, va, _)) = c.anillo {
                anillo(va, A_DUERME).store(1, SeqCst);
            }
        }
        core::sync::atomic::fence(SeqCst);
        let mut quedaba = 0;
        for c in clientes.iter().flatten() {
            quedaba += atender_anillo(ctl, c, nombre);
        }
        if quedaba > 0 {
            continue;
        }
        let (r, k, marca) = crate::canal_leer_marca(canal, &mut buf, &mut ll);
        for c in clientes.iter().flatten() {
            if let Some((_, va, _)) = c.anillo {
                anillo(va, A_DUERME).store(0, SeqCst);
            }
        }
        if r == E_CERRADO {
            return 0;
        }
        if r < 0 {
            crate::dormir_us(10_000);
            continue;
        }
        for &l in ll.iter().take(k).skip(1) {
            crate::cerrar(l);
        }
        if k == 0 {
            // Una entrada del directorio de `inicio` (24 bytes sin marca), o "mira mi anillo".
            if r == 24 && marca == 0 {
                let m = u64::from_le_bytes(buf[..8].try_into().unwrap());
                if let Some(e) = nombres.iter_mut().find(|e| e.0 == 0 || e.0 == m) {
                    e.0 = m;
                    e.1.copy_from_slice(&buf[8..24]);
                }
            }
            continue;
        }
        let respuesta = ll[0];
        let u32_en = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        let (orden_, cuantos, primero, off) = (u32_en(0), u32_en(4) as u64, u32_en(8) as u64 | (u32_en(12) as u64) << 32, u32_en(16) as usize);
        let quien = nombre_de(&nombres, marca);
        let mut ventana = None;
        let estado = if r != 24 || marca == 0 {
            MAL_FORMADO
        } else if orden_ == PEDIR_ABRIR {
            // El disco, en el campo del primer sector.
            let d = primero as usize;
            let tramo = match quien {
                Some(INSTALADOR) => (d < ctl.discos()).then(|| (0, ctl.sectores(d))),
                _ => quien.and_then(|q| tramos.get(d).and_then(|t| t[q])).filter(|_| d < ctl.discos()),
            };
            match tramo {
                None if d >= ctl.discos() && quien.is_some() => FUERA_DEL_DISCO,
                None => SIN_PERMISO,
                Some(tramo) => {
                    let hueco = clientes.iter().position(|c| c.is_some_and(|c| c.marca == marca)).or_else(|| clientes.iter().position(|c| c.is_none()));
                    match hueco {
                        None => SIN_SITIO,
                        Some(h) => {
                            if clientes[h].is_none() {
                                let (m, bus) = crate::memoria_dma(permiso, VENTANA);
                                if m > 0 {
                                    let solo_leer = quien == Some(SOLO_LEER);
                                    clientes[h] = Some(Cliente { marca, llave: m as u32, bus, anillo: None, disco: d, tramo, solo_leer });
                                }
                            }
                            match clientes[h].as_mut() {
                                Some(c) => {
                                    c.disco = d;
                                    c.tramo = tramo;
                                    let copia = crate::llave_duplicar(c.llave, crate::LEER | crate::ESCRIBIR | crate::TRANSFERIR, 0);
                                    ventana = (copia > 0).then_some(copia as u32);
                                    if ventana.is_some() { 0 } else { SIN_SITIO }
                                }
                                None => SIN_SITIO,
                            }
                        }
                    }
                }
            }
        } else if matches!(orden_, PEDIR_LEER | PEDIR_ESCRIBIR | PEDIR_VACIAR | PEDIR_DESCARTAR | PEDIR_LEER_SIN_ESPERA) {
            match clientes.iter().flatten().find(|c| c.marca == marca).copied() {
                None => MAL_FORMADO,
                Some(c) => transferir(ctl, &c, orden_, cuantos, primero, off, nombre),
            }
        } else if orden_ == PEDIR_ANILLO {
            match clientes.iter().position(|c| c.is_some_and(|c| c.marca == marca)) {
                None => MAL_FORMADO,
                Some(h) => {
                    // Uno que ya tenía anillo (relanzado, o que empieza de nuevo): el viejo se suelta (A15).
                    if let Some((m_viejo, va_viejo, aviso_viejo)) = clientes[h].and_then(|c| c.anillo) {
                        crate::memoria_quitar(va_viejo);
                        crate::cerrar(m_viejo);
                        crate::cerrar(aviso_viejo);
                        if let Some(c) = clientes[h].as_mut() {
                            c.anillo = None;
                        }
                    }
                    let m = crate::memoria_crear(4096);
                    let va = VA_ANILLOS + 4096 * h;
                    if m > 0 && crate::memoria_proyectar(m as u32, va, true) == 0 {
                        let copia = crate::llave_duplicar(m as u32, crate::LEER | crate::ESCRIBIR | crate::TRANSFERIR, 0);
                        ventana = (copia > 0).then_some(copia as u32);
                        let aviso = crate::llave_duplicar(respuesta, crate::ESCRIBIR, 0);
                        if let Some(c) = clientes[h].as_mut() {
                            c.anillo = Some((m as u32, va, aviso as u32));
                        }
                        if ventana.is_some() && aviso > 0 { 0 } else { SIN_SITIO }
                    } else {
                        SIN_SITIO
                    }
                }
            }
        } else {
            MAL_FORMADO
        };
        // Los sectores de su tramo y el tamaño del sector de su disco (el que abrió; si no, el 0).
        let (suyos, tam) = match clientes.iter().flatten().find(|c| c.marca == marca) {
            Some(c) => (c.tramo.1 - c.tramo.0, ctl.tam_sector(c.disco)),
            None => (0, if ctl.discos() > 0 { ctl.tam_sector(0) } else { 512 }),
        };
        let mut rsp = [0u8; 24];
        rsp[..4].copy_from_slice(&estado.to_le_bytes());
        rsp[8..16].copy_from_slice(&suyos.to_le_bytes());
        rsp[16..20].copy_from_slice(&(tam as u32).to_le_bytes());
        rsp[20..24].copy_from_slice(&(VENTANA as u32).to_le_bytes());
        let llaves: &[u32] = match &ventana {
            Some(v) => core::slice::from_ref(v),
            None => &[],
        };
        if crate::canal_escribir(respuesta, &rsp, llaves) != 0 {
            if let Some(v) = ventana {
                crate::cerrar(v);
                rsp[..4].copy_from_slice(&SIN_SITIO.to_le_bytes());
                crate::canal_escribir(respuesta, &rsp, &[]);
            }
        }
        crate::cerrar(respuesta);
    }
}
