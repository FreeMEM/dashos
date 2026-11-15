//! El controlador de virtio-gpu (multimonitor, primera versión), fuera del kernel. Escrito con la especificación virtio
//! 1.2: transporte PCI (4.1), colas partidas (2.7) y el aparato de GPU (5.7), solo en 2D (sin VIRGL ni EDID).
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 4); la 3, su espacio de configuración del PCIe; la 4, su
//! interrupción, si la hay (no la usa: mira la cola al mandar); la siguiente, el canal "ventanas" (el compositor).
//!
//! Lee las cabezas que ofrece el aparato (`GET_DISPLAY_INFO`); a cada una le crea un recurso 2D en B8G8R8X8 (el
//! XRGB8888 del compositor), le da memoria DMA en trozos de 4 MB, la pone en su salida y se la pasa al compositor
//! (`protocolo::pantallas`). Después atiende sus `VACIAR`: por cada rectángulo, `TRANSFER_TO_HOST_2D` y
//! `RESOURCE_FLUSH`, todo lo de un mensaje de una vez en la cola.
//!
//! Una cabeza sin monitor (en QEMU sin ventana, todas menos la primera) se usa igual, a `ANCHO_SIN` x `ALTO_SIN`: así
//! se prueba el multimonitor con `-display none`. Lo que llega del compositor no se cree: cabeza y rectángulos se
//! recortan a lo que hay. Lo que devuelve el aparato tampoco: una cabeza de la cola usada que no es nuestra se cuenta y
//! se tira.

#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use dashos::println;
use dashos::protocolo::pantallas as p;

dashos::programa!(principal);

const VA: usize = 0x0090_0000;
const VA_CFG: usize = 0x00A0_0000;
const VA_DMA: usize = 0x30_0000_0000;
const TAM_DMA: usize = 16 * 1024;

// La memoria DMA de control: la cola, las ranuras de las órdenes y la respuesta de GET_DISPLAY_INFO.
const M_COLA: usize = 0x0000;
const M_RANURAS: usize = 0x1000;
const RANURA: usize = 256;
/// Dentro de cada ranura, la respuesta va detrás de la orden.
const RESPUESTA: usize = 192;
const RANURAS: usize = 32;
const M_INFO: usize = 0x3000;

// Cola partida (2.7) de `ENTRADAS`: descriptores de 16 bytes, el anillo del conductor desde +0x400 y el del aparato
// desde +0x800.
const ENTRADAS: u16 = 64;
const DISP: usize = 0x400;
const USADOS: usize = 0x800;
const D_SIGUE: u16 = 1;
const D_ESCRIBE: u16 = 2;

// Registros comunes (4.1.4.3).
const C_FUNC_ELEGIR: usize = 0x00;
const C_FUNC: usize = 0x04;
const C_PROPIAS_ELEGIR: usize = 0x08;
const C_PROPIAS: usize = 0x0C;
const C_ESTADO: usize = 0x14;
const C_COLA_ELEGIR: usize = 0x16;
const C_COLA_TAMANO: usize = 0x18;
const C_COLA_ENCENDER: usize = 0x1C;
const C_COLA_AVISO: usize = 0x1E;
const C_COLA_DESC: usize = 0x20;
const C_COLA_DISP: usize = 0x28;
const C_COLA_USADOS: usize = 0x30;
const S_ENTERADO: u8 = 1;
const S_CONDUCTOR: u8 = 2;
const S_LISTO: u8 = 4;
const S_FUNC_OK: u8 = 8;
const S_FALLO: u8 = 128;

// El aparato de GPU (5.7.6): órdenes y respuestas.
const COLA_CONTROL: u16 = 0;
const GET_DISPLAY_INFO: u32 = 0x0100;
const RESOURCE_CREATE_2D: u32 = 0x0101;
const SET_SCANOUT: u32 = 0x0103;
const RESOURCE_FLUSH: u32 = 0x0104;
const TRANSFER_TO_HOST_2D: u32 = 0x0105;
const RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const OK_NODATA: u32 = 0x1100;
const OK_DISPLAY_INFO: u32 = 0x1101;
/// B8G8R8X8_UNORM: en memoria B, G, R, X; el píxel 0x00RRGGBB en little-endian.
const FORMATO: u32 = 2;
/// La cabecera de cada orden (`virtio_gpu_ctrl_hdr`): tipo, flags, fence (2 palabras), contexto y anillo: 6 palabras.
const CAB: usize = 6;
/// En la configuración del aparato (5.7.4): cuántas salidas.
const CFG_SALIDAS: usize = 8;
const MAX_SALIDAS: usize = 16;

/// El tamaño de una cabeza sin monitor.
const ANCHO_SIN: u32 = 1024;
const ALTO_SIN: u32 = 768;

fn reg8(off: usize) -> u8 {
    unsafe { read_volatile((VA + off) as *const u8) }
}
fn poner_reg8(off: usize, v: u8) {
    unsafe { write_volatile((VA + off) as *mut u8, v) }
}
fn reg16(off: usize) -> u16 {
    u16::from_le(unsafe { read_volatile((VA + off) as *const u16) })
}
fn poner_reg16(off: usize, v: u16) {
    unsafe { write_volatile((VA + off) as *mut u16, v.to_le()) }
}
fn reg32(off: usize) -> u32 {
    u32::from_le(unsafe { read_volatile((VA + off) as *const u32) })
}
fn poner_reg32(off: usize, v: u32) {
    unsafe { write_volatile((VA + off) as *mut u32, v.to_le()) }
}
fn poner_reg64(off: usize, v: u64) {
    poner_reg32(off, v as u32);
    poner_reg32(off + 4, (v >> 32) as u32);
}
fn cfg32(off: usize) -> u32 {
    u32::from_le(unsafe { read_volatile((VA_CFG + off) as *const u32) })
}
fn dma16(off: usize) -> u16 {
    u16::from_le(unsafe { read_volatile((VA_DMA + off) as *const u16) })
}
fn dma32(off: usize) -> u32 {
    u32::from_le(unsafe { read_volatile((VA_DMA + off) as *const u32) })
}
fn poner16(off: usize, v: u16) {
    unsafe { write_volatile((VA_DMA + off) as *mut u16, v.to_le()) }
}
fn poner32(off: usize, v: u32) {
    unsafe { write_volatile((VA_DMA + off) as *mut u32, v.to_le()) }
}
fn poner64(off: usize, v: u64) {
    poner32(off, v as u32);
    poner32(off + 4, (v >> 32) as u32);
}

/// Dónde están los bloques de registros dentro de la BAR (4.1.4): comunes, avisos (con su multiplicador) y los del
/// aparato.
struct Bloques {
    comun: usize,
    aviso: usize,
    multiplicador: u32,
    aparato: usize,
}

/// Recorre las capacidades del PCIe buscando las de virtio (0x09); todas deben estar en la BAR 4, la que da el kernel.
fn bloques() -> Option<Bloques> {
    if (cfg32(4) >> 16) & (1 << 4) == 0 {
        return None;
    }
    let (mut comun, mut aviso, mut aparato, mut multiplicador) = (None, None, None, 0);
    let mut q = (cfg32(0x34) & 0xfc) as usize;
    let mut vueltas = 0;
    while q != 0 && vueltas < 48 {
        let w = cfg32(q);
        if w & 0xff == 0x09 && cfg32(q + 4) & 0xff == 4 {
            let desde = cfg32(q + 8) as usize;
            match (w >> 24) & 0xff {
                1 => comun = Some(desde),
                2 => {
                    aviso = Some(desde);
                    multiplicador = cfg32(q + 16);
                }
                4 => aparato = Some(desde),
                _ => {}
            }
        }
        q = ((w >> 8) & 0xfc) as usize;
        vueltas += 1;
    }
    Some(Bloques { comun: comun?, aviso: aviso?, multiplicador, aparato: aparato? })
}

/// La cola de control en la memoria DMA: su dirección de bus, su aviso y lo que lleva el conductor.
struct Cola {
    bus: u64,
    aviso: usize,
    disp: u16,
    usados: u16,
    /// Cabezas devueltas que no eran nuestras (el aparato no se cree).
    raras: u64,
}

impl Cola {
    /// Pone el descriptor `i`: `off` y `largo` en la memoria DMA.
    fn descriptor(&self, i: u16, off: usize, largo: u32, flags: u16, siguiente: u16) {
        let d = M_COLA + 16 * i as usize;
        poner64(d, self.bus - M_COLA as u64 + off as u64);
        poner32(d + 8, largo);
        poner16(d + 12, flags);
        poner16(d + 14, siguiente);
    }

    /// Manda las órdenes que hay en las ranuras `0..n` (cada una con `largos[k]` bytes y una respuesta de
    /// `respuestas[k]`) y espera a que vuelvan todas. Devuelve cuántas dijeron que bien (`OK_NODATA` u `ok`).
    fn mandar(&mut self, largos: &[u32], respuestas: &[u32], ok: u32) -> usize {
        let n = largos.len().min(RANURAS);
        for k in 0..n {
            let ranura = M_RANURAS + k * RANURA;
            let resp = if respuestas[k] > (RANURA - RESPUESTA) as u32 { M_INFO } else { ranura + RESPUESTA };
            poner32(resp, 0);
            self.descriptor(2 * k as u16, ranura, largos[k], D_SIGUE, 2 * k as u16 + 1);
            self.descriptor(2 * k as u16 + 1, resp, respuestas[k], D_ESCRIBE, 0);
            poner16(M_COLA + DISP + 4 + 2 * (self.disp.wrapping_add(k as u16) % ENTRADAS) as usize, 2 * k as u16);
        }
        unsafe { core::arch::asm!("dmb oshst") };
        self.disp = self.disp.wrapping_add(n as u16);
        poner16(M_COLA + DISP + 2, self.disp);
        unsafe { core::arch::asm!("dmb osh") };
        poner_reg16(self.aviso, COLA_CONTROL);
        // Se espera a que vuelvan: primero girando un poco y después a ratos de 100 µs, hasta 1 s.
        let fin = dashos::ahora_us() + 1_000_000;
        let mut vueltas = 0u32;
        let mut bien = 0;
        let mut vuelto = 0;
        while vuelto < n && dashos::ahora_us() < fin {
            if dma16(M_COLA + USADOS + 2) == self.usados {
                vueltas += 1;
                if vueltas > 2_000 {
                    dashos::dormir_us(100);
                } else {
                    core::hint::spin_loop();
                }
                continue;
            }
            unsafe { core::arch::asm!("dmb oshld") };
            let id = dma32(M_COLA + USADOS + 4 + 8 * (self.usados % ENTRADAS) as usize) as usize;
            self.usados = self.usados.wrapping_add(1);
            if id % 2 != 0 || id / 2 >= n {
                self.raras += 1;
                continue;
            }
            vuelto += 1;
            let k = id / 2;
            let resp = if respuestas[k] > (RANURA - RESPUESTA) as u32 { M_INFO } else { M_RANURAS + k * RANURA + RESPUESTA };
            let tipo = dma32(resp);
            if tipo == OK_NODATA || tipo == ok {
                bien += 1;
            }
        }
        bien
    }
}

/// Monta la cola de control (2.7, 4.1.5.1.3).
fn montar(b: &Bloques, bus: u64) -> Option<Cola> {
    poner_reg16(b.comun + C_COLA_ELEGIR, COLA_CONTROL);
    let max = reg16(b.comun + C_COLA_TAMANO);
    if max < ENTRADAS {
        return None;
    }
    poner_reg16(b.comun + C_COLA_TAMANO, ENTRADAS);
    let base = bus + M_COLA as u64;
    poner_reg64(b.comun + C_COLA_DESC, base);
    poner_reg64(b.comun + C_COLA_DISP, base + DISP as u64);
    poner_reg64(b.comun + C_COLA_USADOS, base + USADOS as u64);
    let aviso = b.aviso + reg16(b.comun + C_COLA_AVISO) as usize * b.multiplicador as usize;
    poner_reg16(b.comun + C_COLA_ENCENDER, 1);
    Some(Cola { bus: base, aviso, disp: 0, usados: 0, raras: 0 })
}

/// Escribe una orden en la ranura `k`: la cabecera con su tipo y después `datos`. Devuelve su largo en bytes.
fn orden(k: usize, tipo: u32, datos: &[u32]) -> u32 {
    let r = M_RANURAS + k * RANURA;
    poner32(r, tipo);
    for i in 1..CAB {
        poner32(r + 4 * i, 0);
    }
    for (i, w) in datos.iter().enumerate() {
        poner32(r + 4 * (CAB + i), *w);
    }
    (4 * (CAB + datos.len())) as u32
}

/// Una cabeza: su tamaño, su recurso y su memoria (llaves y direcciones de bus de los trozos).
struct Cabeza {
    ancho: u32,
    alto: u32,
    conectada: bool,
    trozos: [(u32, u64, u32); p::MAX_TROZOS],
    n_trozos: usize,
}

fn principal() -> u64 {
    // Con llave 5, la 4 es la interrupción y la 5 el canal "ventanas"; si no, el canal es la 4.
    let existe = |k: u32| {
        let copia = dashos::llave_duplicar(k, dashos::LEER, 0);
        if copia > 0 {
            dashos::cerrar(copia as u32);
        }
        copia > 0
    };
    let ventanas = if existe(5) { 5u32 } else { 4u32 };
    if dashos::registros_proyectar(2, VA) != 0 || dashos::registros_proyectar(3, VA_CFG) != 0 {
        println!("  [virtio-gpu] sin registros");
        return 1;
    }
    let Some(b) = bloques() else {
        println!("  [virtio-gpu] sin las capacidades de virtio en la BAR 4");
        return 2;
    };
    // Reiniciar, enterado, conductor (3.1.1).
    poner_reg8(b.comun + C_ESTADO, 0);
    let fin = dashos::ahora_us() + 100_000;
    while reg8(b.comun + C_ESTADO) != 0 && dashos::ahora_us() < fin {}
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO);
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO | S_CONDUCTOR);
    // Solo VIRTIO_F_VERSION_1 (el bit 32): sin VIRGL ni EDID.
    poner_reg32(b.comun + C_FUNC_ELEGIR, 1);
    if reg32(b.comun + C_FUNC) & 1 == 0 {
        println!("  [virtio-gpu] sin la versión 1");
        return 3;
    }
    poner_reg32(b.comun + C_PROPIAS_ELEGIR, 0);
    poner_reg32(b.comun + C_PROPIAS, 0);
    poner_reg32(b.comun + C_PROPIAS_ELEGIR, 1);
    poner_reg32(b.comun + C_PROPIAS, 1);
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO | S_CONDUCTOR | S_FUNC_OK);
    if reg8(b.comun + C_ESTADO) & S_FUNC_OK == 0 {
        println!("  [virtio-gpu] no acepta las funciones");
        return 4;
    }
    // Lo de un controlador anterior que murió a medias (el aparato ya está reiniciado).
    dashos::dma_liberar(1);
    let (m, bus) = dashos::memoria_dma(1, TAM_DMA);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_DMA, true) != 0 {
        println!("  [virtio-gpu] sin memoria DMA");
        return 5;
    }
    for i in (0..TAM_DMA).step_by(4) {
        poner32(i, 0);
    }
    let Some(mut cola) = montar(&b, bus) else {
        println!("  [virtio-gpu] no monta la cola de control");
        return 6;
    };
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO | S_CONDUCTOR | S_FUNC_OK | S_LISTO);
    if reg8(b.comun + C_ESTADO) & S_FALLO != 0 {
        println!("  [virtio-gpu] el aparato falla al arrancar");
        return 7;
    }
    let salidas = (reg32(b.aparato + CFG_SALIDAS) as usize).min(MAX_SALIDAS);
    // Las cabezas (5.7.6.8): 16 de 24 bytes (rectángulo, encendida, flags) tras la cabecera.
    let largo = orden(0, GET_DISPLAY_INFO, &[]);
    if cola.mandar(&[largo], &[(4 * CAB + 24 * MAX_SALIDAS) as u32], OK_DISPLAY_INFO) != 1 {
        println!("  [virtio-gpu] GET_DISPLAY_INFO falla");
        return 8;
    }
    let n = salidas.clamp(1, p::MAX_CABEZAS);
    println!("  [virtio-gpu] {} salidas; se usan {}", salidas, n);
    let mut cabezas: [Option<Cabeza>; p::MAX_CABEZAS] = [const { None }; p::MAX_CABEZAS];
    for (i, cab) in cabezas.iter_mut().enumerate().take(n) {
        let info = M_INFO + 4 * CAB + 24 * i;
        let (ancho, alto, encendida) = (dma32(info + 8), dma32(info + 12), dma32(info + 16) != 0);
        let conectada = encendida && (1..=p::MAX_LADO).contains(&ancho) && (1..=p::MAX_LADO).contains(&alto);
        let (ancho, alto) = if conectada { (ancho, alto) } else { (ANCHO_SIN, ALTO_SIN) };
        // La memoria, en trozos de 4 MB (lo más que da `memoria_dma`).
        let total = (ancho * alto * 4) as usize;
        let n_trozos = total.div_ceil(p::TROZO);
        if n_trozos > p::MAX_TROZOS {
            println!("  [virtio-gpu] cabeza {}: {}x{} no cabe", i, ancho, alto);
            continue;
        }
        let mut c = Cabeza { ancho, alto, conectada, trozos: [(0, 0, 0); p::MAX_TROZOS], n_trozos };
        let mut bien = true;
        for k in 0..n_trozos {
            let largo = (total - k * p::TROZO).min(p::TROZO);
            let (llave, bus) = dashos::memoria_dma(1, largo);
            if llave <= 0 {
                bien = false;
                break;
            }
            c.trozos[k] = (llave as u32, bus, largo as u32);
        }
        if !bien {
            println!("  [virtio-gpu] cabeza {}: sin memoria DMA para {}x{}", i, ancho, alto);
            continue;
        }
        // El recurso (5.7.6.9), su memoria (5.7.6.13), en su salida (5.7.6.10) y entero en negro (5.7.6.11, 5.7.6.12).
        let recurso = i as u32 + 1;
        let l0 = orden(0, RESOURCE_CREATE_2D, &[recurso, FORMATO, ancho, alto]);
        let mut entradas = [0u32; 2 + 4 * p::MAX_TROZOS];
        entradas[0] = recurso;
        entradas[1] = n_trozos as u32;
        for k in 0..n_trozos {
            let (_, bus, largo) = c.trozos[k];
            entradas[2 + 4 * k] = bus as u32;
            entradas[3 + 4 * k] = (bus >> 32) as u32;
            entradas[4 + 4 * k] = largo;
        }
        let l1 = orden(1, RESOURCE_ATTACH_BACKING, &entradas[..2 + 4 * n_trozos]);
        let l2 = orden(2, SET_SCANOUT, &[0, 0, ancho, alto, i as u32, recurso]);
        let l3 = orden(3, TRANSFER_TO_HOST_2D, &[0, 0, ancho, alto, 0, 0, recurso, 0]);
        let l4 = orden(4, RESOURCE_FLUSH, &[0, 0, ancho, alto, recurso, 0]);
        let bien = cola.mandar(&[l0, l1, l2, l3, l4], &[24; 5], OK_NODATA);
        if bien != 5 {
            println!("  [virtio-gpu] cabeza {}: el aparato rechaza {} de 5 órdenes", i, 5 - bien);
            continue;
        }
        println!("  [virtio-gpu] cabeza {}: {}x{}, {}, en {} trozos", i, ancho, alto, if conectada { "conectada" } else { "sin monitor (se usa igual)" }, n_trozos);
        *cab = Some(c);
    }
    let usadas: usize = cabezas.iter().filter(|c| c.is_some()).count();
    if usadas == 0 {
        println!("  [virtio-gpu] ninguna cabeza en marcha");
        return 9;
    }
    // Al compositor: cada cabeza con su memoria; con la primera, el canal por el que pide vaciar.
    let (lee, escribe) = dashos::canal_crear();
    let mut numero = 0u32;
    for c in cabezas.iter().flatten() {
        let mut llaves = [0u32; 8];
        for k in 0..c.n_trozos {
            let copia = dashos::llave_duplicar(c.trozos[k].0, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
            llaves[k] = copia.max(0) as u32;
        }
        let mut n_llaves = c.n_trozos;
        if numero == 0 {
            llaves[n_llaves] = escribe;
            n_llaves += 1;
        }
        let m = [p::CABEZA, numero, usadas as u32, c.ancho, c.alto, c.ancho * 4, c.n_trozos as u32, c.conectada as u32];
        // Con la cola del compositor llena (al arrancar le escriben todos; `E_LIMITE`, -8), se reintenta un rato.
        let mut r = 0;
        for _ in 0..500 {
            r = dashos::canal_escribir(ventanas, palabras(&m), &llaves[..n_llaves]);
            if r != -8 {
                break;
            }
            dashos::dormir_us(10_000);
        }
        if r != 0 {
            println!("  [virtio-gpu] el compositor no recibe la cabeza {}: {}", numero, r);
            return 10;
        }
        numero += 1;
    }
    // Los recursos, por el número que ve el compositor (el de la cabeza en marcha), y su tamaño.
    let mut recursos = [(0u32, 0u32, 0u32); p::MAX_CABEZAS];
    for (k, (i, c)) in cabezas.iter().enumerate().filter_map(|(i, c)| c.as_ref().map(|c| (i, c))).enumerate() {
        recursos[k] = (i as u32 + 1, c.ancho, c.alto);
    }
    println!("  [virtio-gpu] {} cabezas para el compositor", usadas);
    let mut buf = [0u8; 256];
    let (mut vaciados, mut malos) = (0u64, 0u64);
    loop {
        let (n, k) = dashos::canal_leer(lee, &mut buf, &mut []);
        if n < 0 {
            println!("  [virtio-gpu] el compositor cerró su canal");
            return 0;
        }
        let _ = k;
        let n = n as usize;
        let w = |i: usize| u32::from_le_bytes(buf[4 * i..4 * i + 4].try_into().unwrap());
        if n < 12 || w(0) != p::VACIAR {
            malos += 1;
            continue;
        }
        let cabeza = w(1) as usize;
        let rects = (w(2) as usize).min(p::MAX_RECTS).min((n - 12) / 8);
        if cabeza >= usadas {
            malos += 1;
            continue;
        }
        let (recurso, ancho, alto) = recursos[cabeza];
        // Por cada rectángulo, dentro de la cabeza: transferir y vaciar. De 16 en 16 (32 ranuras).
        let mut largos = [0u32; RANURAS];
        let mut r = 0;
        while r < rects {
            let mut k = 0;
            while r < rects && k + 1 < RANURAS {
                let h = |j: usize| u16::from_le_bytes(buf[12 + 8 * r + 2 * j..14 + 8 * r + 2 * j].try_into().unwrap()) as u32;
                let (x, y) = (h(0).min(ancho), h(1).min(alto));
                let (rw, rh) = (h(2).min(ancho - x), h(3).min(alto - y));
                r += 1;
                if rw == 0 || rh == 0 {
                    continue;
                }
                let desde = (y as u64) * (ancho as u64 * 4) + x as u64 * 4;
                largos[k] = orden(k, TRANSFER_TO_HOST_2D, &[x, y, rw, rh, desde as u32, (desde >> 32) as u32, recurso, 0]);
                largos[k + 1] = orden(k + 1, RESOURCE_FLUSH, &[x, y, rw, rh, recurso, 0]);
                k += 2;
            }
            if k > 0 && cola.mandar(&largos[..k], &[24; RANURAS][..k], OK_NODATA) != k {
                malos += 1;
            }
        }
        vaciados += 1;
        if vaciados % 2000 == 0 {
            println!("  [virtio-gpu] {} vaciados, {} malos, {} raros", vaciados, malos, cola.raras);
        }
    }
}

fn palabras(p: &[u32]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(p.as_ptr() as *const u8, 4 * p.len()) }
}
