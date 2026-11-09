//! El controlador de virtio-sound (S19, A3), fuera del kernel. Escrito con la especificación virtio 1.2: transporte PCI
//! (4.1: capacidades de configuración, registros comunes, avisos), colas partidas (2.7) y el aparato de sonido (5.14).
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR de virtio); la 3, su espacio de configuración del PCIe,
//! para leer dónde está cada bloque de registros; la 4, su interrupción, si la hay; la siguiente, el canal "sonido"
//! (el servicio, A4). Espera a su interrupción, que llega cada vez que el aparato devuelve un búfer (A6); sin ella,
//! mira las colas con un temporizador (que va en tics de 10 ms: con 8 búferes de 5,3 ms, aguanta).
//!
//! Lee las capacidades, reinicia el aparato, acuerda la versión 1, monta la cola de control y la de salida, busca un
//! flujo PCM de salida que admita estéreo de 16 bits a 48 kHz, lo prepara con períodos de 256 cuadros (1 KB) y un búfer
//! de 8, y suena lo que mezcla el servicio "sonido" (A4): le da una memoria con ranuras de un período, y le avisa
//! cada vez que el aparato devuelve un búfer. Toda su memoria DMA es del permiso, sin caché.

#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x0090_0000;
const VA_CFG: usize = 0x00A0_0000;
const VA_DMA: usize = 0x30_0000_0000;
const TAM_DMA: usize = 64 * 1024;

// La memoria DMA: cada cola en su página, las peticiones de control y los búferes de salida.
const M_CONTROL: usize = 0x0000;
const M_SALIDA: usize = 0x1000;
const M_PETICION: usize = 0x2000;
const M_RESPUESTA: usize = 0x2100;
const M_BUFERES: usize = 0x4000;
/// Cada búfer de salida: la cabecera (el flujo) en +0x3C, justo antes de los datos en +0x40; el estado en +0x10.
const TAM_RANURA: usize = 0x800;
const CABECERA: usize = 0x3C;
const ESTADO: usize = 0x10;
const DATOS: usize = 0x40;

// Cola partida (2.7) de `ENTRADAS`: descriptores de 16 bytes, el anillo del conductor desde +0x100 y el del aparato
// desde +0x200.
const ENTRADAS: u16 = 16;
const DISP: usize = 0x100;
const USADOS: usize = 0x200;
const D_SIGUE: u16 = 1;
const D_ESCRIBE: u16 = 2;

// Registros comunes (4.1.4.3).
const C_FUNC_ELEGIR: usize = 0x00;
const C_FUNC: usize = 0x04;
const C_PROPIAS_ELEGIR: usize = 0x08;
const C_PROPIAS: usize = 0x0C;
const C_COLAS: usize = 0x12;
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

// El aparato de sonido (5.14).
const COLA_CONTROL: u16 = 0;
const COLA_SALIDA: u16 = 2;
const R_PCM_INFO: u32 = 0x0100;
const R_PCM_PARAMETROS: u32 = 0x0101;
const R_PCM_PREPARAR: u32 = 0x0102;
const R_PCM_EMPEZAR: u32 = 0x0104;
const BIEN: u32 = 0x8000;
const FORMATO_S16: u8 = 5;
const RITMO_48000: u8 = 7;
const SALIDA_PCM: u8 = 0;

// La salida.
const CUADROS: usize = 256;
const PERIODO: usize = CUADROS * 4;
const BUFERES: usize = 8;
/// Las ranuras de la salida que llena el servicio.
const RANURAS: usize = 8;
const VA_SALIDA: usize = 0x40_0000_0000;
/// Lo que se manda al servicio (`usuario/src/bin/sonido.rs`).
const P_SALIDA: u32 = 1;
const P_TIC: u32 = 2;

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

/// Dónde están los bloques de registros dentro de la BAR (4.1.4): comunes, estado de la interrupción, avisos (con su
/// multiplicador) y los del aparato.
struct Bloques {
    comun: usize,
    isr: usize,
    aviso: usize,
    multiplicador: u32,
    aparato: usize,
}

/// Recorre las capacidades del PCIe buscando las de virtio (0x09); todas deben estar en la BAR 4, la que da el kernel.
fn bloques() -> Option<Bloques> {
    if (cfg32(4) >> 16) & (1 << 4) == 0 {
        return None;
    }
    let (mut comun, mut aviso, mut aparato, mut isr, mut multiplicador) = (None, None, None, None, 0);
    let mut p = (cfg32(0x34) & 0xfc) as usize;
    let mut vueltas = 0;
    while p != 0 && vueltas < 48 {
        let w = cfg32(p);
        if w & 0xff == 0x09 {
            let tipo = (w >> 24) & 0xff;
            let bar = cfg32(p + 4) & 0xff;
            let desde = cfg32(p + 8) as usize;
            if bar == 4 {
                match tipo {
                    1 => comun = Some(desde),
                    2 => {
                        aviso = Some(desde);
                        multiplicador = cfg32(p + 16);
                    }
                    3 => isr = Some(desde),
                    4 => aparato = Some(desde),
                    _ => {}
                }
            }
        }
        p = ((w >> 8) & 0xfc) as usize;
        vueltas += 1;
    }
    Some(Bloques { comun: comun?, isr: isr?, aviso: aviso?, multiplicador, aparato: aparato? })
}

/// Una cola partida en la memoria DMA: dónde empieza, su bus, su número y el aviso, y lo que lleva el conductor.
struct Cola {
    m: usize,
    bus: u64,
    aviso: usize,
    disp: u16,
    usados: u16,
}

impl Cola {
    /// Pone el descriptor `i`.
    fn descriptor(&self, i: u16, off: usize, largo: u32, flags: u16, siguiente: u16) {
        let d = self.m + 16 * i as usize;
        poner64(d, self.bus - self.m as u64 + off as u64);
        poner32(d + 8, largo);
        poner16(d + 12, flags);
        poner16(d + 14, siguiente);
    }

    /// Ofrece al aparato la cadena que empieza en el descriptor `cabeza` y le avisa.
    fn ofrecer(&mut self, cabeza: u16, numero: u16) {
        poner16(self.m + DISP + 4 + 2 * (self.disp % ENTRADAS) as usize, cabeza);
        unsafe { core::arch::asm!("dmb oshst") };
        self.disp = self.disp.wrapping_add(1);
        poner16(self.m + DISP + 2, self.disp);
        unsafe { core::arch::asm!("dmb osh") };
        poner_reg16(self.aviso, numero);
    }

    /// La siguiente cadena que el aparato devolvió (su cabeza), si hay.
    fn devuelta(&mut self) -> Option<u16> {
        if dma16(self.m + USADOS + 2) == self.usados {
            return None;
        }
        unsafe { core::arch::asm!("dmb oshld") };
        let id = dma32(self.m + USADOS + 4 + 8 * (self.usados % ENTRADAS) as usize);
        self.usados = self.usados.wrapping_add(1);
        Some(id as u16)
    }
}

/// Monta la cola `numero` en `m` (2.7, 4.1.5.1.3).
fn montar(b: &Bloques, bus: u64, numero: u16, m: usize) -> Option<Cola> {
    poner_reg16(b.comun + C_COLA_ELEGIR, numero);
    let max = reg16(b.comun + C_COLA_TAMANO);
    if max < ENTRADAS {
        return None;
    }
    poner_reg16(b.comun + C_COLA_TAMANO, ENTRADAS);
    let base = bus + m as u64;
    poner_reg64(b.comun + C_COLA_DESC, base);
    poner_reg64(b.comun + C_COLA_DISP, base + DISP as u64);
    poner_reg64(b.comun + C_COLA_USADOS, base + USADOS as u64);
    let aviso = b.aviso + reg16(b.comun + C_COLA_AVISO) as usize * b.multiplicador as usize;
    poner_reg16(b.comun + C_COLA_ENCENDER, 1);
    Some(Cola { m, bus: base, aviso, disp: 0, usados: 0 })
}

/// Una petición de control: `peticion` en la página de peticiones, la respuesta (`largo` bytes) en la suya. Devuelve
/// el estado de la respuesta.
fn control(c: &mut Cola, peticion: &[u32], largo: u32) -> u32 {
    for (i, w) in peticion.iter().enumerate() {
        poner32(M_PETICION + 4 * i, *w);
    }
    poner32(M_RESPUESTA, 0);
    c.descriptor(0, M_PETICION, 4 * peticion.len() as u32, D_SIGUE, 1);
    c.descriptor(1, M_RESPUESTA, largo, D_ESCRIBE, 0);
    c.ofrecer(0, COLA_CONTROL);
    let fin = dashos::ahora_us() + 1_000_000;
    while dashos::ahora_us() < fin {
        if c.devuelta().is_some() {
            return dma32(M_RESPUESTA);
        }
        core::hint::spin_loop();
    }
    0
}

fn principal() -> u64 {
    // Lo primero, antes de crear llaves: con llave 5, la 4 es la interrupción y la 5 el canal "sonido"; si no, el
    // canal es la 4.
    let existe = |k: u32| {
        let copia = dashos::llave_duplicar(k, dashos::LEER, 0);
        if copia > 0 {
            dashos::cerrar(copia as u32);
        }
        copia > 0
    };
    let (irq, servicio) = if existe(5) { (Some(4u32), 5u32) } else { (None, 4u32) };
    if dashos::registros_proyectar(2, VA) != 0 || dashos::registros_proyectar(3, VA_CFG) != 0 {
        println!("  [virtio-sonido] sin registros");
        return 1;
    }
    let Some(b) = bloques() else {
        println!("  [virtio-sonido] sin las capacidades de virtio en la BAR 4");
        return 2;
    };
    // Reiniciar, enterado, conductor (3.1.1).
    poner_reg8(b.comun + C_ESTADO, 0);
    let fin = dashos::ahora_us() + 100_000;
    while reg8(b.comun + C_ESTADO) != 0 && dashos::ahora_us() < fin {}
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO);
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO | S_CONDUCTOR);
    // Solo VIRTIO_F_VERSION_1 (el bit 32), que es obligatoria en PCI moderno.
    poner_reg32(b.comun + C_FUNC_ELEGIR, 1);
    if reg32(b.comun + C_FUNC) & 1 == 0 {
        println!("  [virtio-sonido] sin la versión 1");
        return 3;
    }
    poner_reg32(b.comun + C_PROPIAS_ELEGIR, 0);
    poner_reg32(b.comun + C_PROPIAS, 0);
    poner_reg32(b.comun + C_PROPIAS_ELEGIR, 1);
    poner_reg32(b.comun + C_PROPIAS, 1);
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO | S_CONDUCTOR | S_FUNC_OK);
    if reg8(b.comun + C_ESTADO) & S_FUNC_OK == 0 {
        println!("  [virtio-sonido] no acepta las funciones");
        return 4;
    }
    dashos::dma_liberar(1);
    let (m, bus) = dashos::memoria_dma(1, TAM_DMA);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_DMA, true) != 0 {
        println!("  [virtio-sonido] sin memoria DMA");
        return 5;
    }
    for i in (0..TAM_DMA).step_by(4) {
        poner32(i, 0);
    }
    let colas = reg16(b.comun + C_COLAS);
    let (Some(mut ctl), Some(mut tx)) = (montar(&b, bus, COLA_CONTROL, M_CONTROL), montar(&b, bus, COLA_SALIDA, M_SALIDA)) else {
        println!("  [virtio-sonido] no monta las colas ({} colas)", colas);
        return 6;
    };
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO | S_CONDUCTOR | S_FUNC_OK | S_LISTO);
    if reg8(b.comun + C_ESTADO) & S_FALLO != 0 {
        println!("  [virtio-sonido] el aparato falla al arrancar");
        return 7;
    }
    let (conectores, flujos) = (reg32(b.aparato), reg32(b.aparato + 4));
    println!("  [virtio-sonido] {} colas, {} flujos PCM, {} conectores", colas, flujos, conectores);
    // La información de los flujos (5.14.6.6.2): 32 bytes cada uno tras la cabecera de 4.
    let flujos = flujos.min(8);
    if control(&mut ctl, &[R_PCM_INFO, 0, flujos, 32], 4 + 32 * flujos) != BIEN {
        println!("  [virtio-sonido] PCM_INFO falla");
        return 8;
    }
    let info = |i: u32, off: usize| unsafe { read_volatile((VA_DMA + M_RESPUESTA + 4 + 32 * i as usize + off) as *const u8) };
    let de_salida = (0..flujos).find(|&i| {
        let formatos = dma32(M_RESPUESTA + 4 + 32 * i as usize + 8);
        let ritmos = dma32(M_RESPUESTA + 4 + 32 * i as usize + 16);
        info(i, 24) == SALIDA_PCM && formatos & (1 << FORMATO_S16) != 0 && ritmos & (1 << RITMO_48000) != 0 && info(i, 26) >= 2 && info(i, 25) <= 2
    });
    let Some(flujo) = de_salida else {
        println!("  [virtio-sonido] ningún flujo de salida estéreo de 16 bits a 48 kHz");
        return 9;
    };
    // Parámetros (5.14.6.6.3): búfer y período en bytes, funciones, canales, formato y ritmo.
    let params = [R_PCM_PARAMETROS, flujo, (BUFERES * PERIODO) as u32, PERIODO as u32, 0, 2 | (FORMATO_S16 as u32) << 8 | (RITMO_48000 as u32) << 16];
    if control(&mut ctl, &params, 4) != BIEN || control(&mut ctl, &[R_PCM_PREPARAR, flujo], 4) != BIEN {
        println!("  [virtio-sonido] el flujo {} no se prepara", flujo);
        return 10;
    }
    // La salida que llena el servicio "sonido" (A4): cabecera (escritos, leídos, ranuras, cuadros) y las ranuras desde
    // el byte 64, un período cada una.
    let tam = 64 + RANURAS * PERIODO;
    let mem = dashos::memoria_crear(tam);
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_SALIDA, true) != 0 {
        println!("  [virtio-sonido] sin memoria para la salida");
        return 12;
    }
    poner_salida(2, RANURAS as u32);
    poner_salida(3, CUADROS as u32);
    let copia = dashos::llave_duplicar(mem, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let peticion: [u32; 4] = [P_SALIDA, RANURAS as u32, CUADROS as u32, 48_000];
    if copia <= 0 || dashos::canal_escribir(servicio, palabras(&peticion), &[copia as u32]) != 0 {
        println!("  [virtio-sonido] el servicio no recibe la salida");
        return 13;
    }
    // Cada búfer, dos descriptores: cabecera y datos (que el aparato lee) y el estado (que escribe). Empiezan en
    // silencio.
    for r in 0..BUFERES {
        let base = M_BUFERES + r * TAM_RANURA;
        tx.descriptor(2 * r as u16, base + CABECERA, (4 + PERIODO) as u32, D_SIGUE, 2 * r as u16 + 1);
        tx.descriptor(2 * r as u16 + 1, base + ESTADO, 8, D_ESCRIBE, 0);
        poner32(base + CABECERA, flujo);
        tx.ofrecer(2 * r as u16, COLA_SALIDA);
    }
    if control(&mut ctl, &[R_PCM_EMPEZAR, flujo], 4) != BIEN {
        println!("  [virtio-sonido] el flujo {} no empieza", flujo);
        return 11;
    }
    println!("  [virtio-sonido] suena el flujo {} para el servicio", flujo);
    // La clase de plazo (S2, A6): hasta 1 ms de CPU cada período de 5,3 ms.
    if dashos::plazo(1_000, 5_333) != 0 {
        println!("  [virtio-sonido] sin clase de plazo: va en la justa");
    }
    // Cada búfer que vuelve se llena con el siguiente período del servicio (o silencio, si no lo tiene: un vacío) y se
    // avisa al servicio con un tic. Se mira la cola cada 2 ms (un período son 5,3).
    let (mut devueltos, mut vacios, mut errores) = (0u64, 0u64, 0u64);
    let mut sonando = false;
    // La interrupción, a un canal propio. Por nivel (INTx): leer el estado (ISR) la baja, y después se vuelve a
    // habilitar (4.1.4.5).
    let avisos = irq.and_then(|i| {
        let (lee, escribe) = dashos::canal_crear();
        (dashos::interrupcion_asociar(i, escribe) == 0).then_some((lee, i))
    });
    println!("  [virtio-sonido] avisos {}", if avisos.is_some() { "por su interrupción" } else { "con un temporizador" });
    let empiezo = dashos::ahora_us();
    let mut antes = empiezo;
    loop {
        match avisos {
            Some((lee, i)) => {
                let mut m8 = [0u8; 8];
                let mut ll = [0u32; 1];
                dashos::canal_leer(lee, &mut m8, &mut ll);
                reg8(b.isr);
                dashos::interrupcion_terminar(i);
            }
            None => dashos::dormir_us(2000),
        }
        // Medida de A6: si entre dos vueltas pasan más de 15 ms (lo normal son 2), decirlo.
        let ahora = dashos::ahora_us();
        if ahora - antes > 15_000 {
            println!("  [virtio-sonido] parado {} ms, a los {} ms", (ahora - antes) / 1000, (antes - empiezo) / 1000);
        }
        antes = ahora;
        let mut alguno = false;
        while let Some(cabeza) = tx.devuelta() {
            let r = (cabeza / 2) as usize;
            let base = M_BUFERES + r * TAM_RANURA;
            if dma32(base + ESTADO) != BIEN {
                errores += 1;
            }
            poner32(base + ESTADO, 0);
            let (escritos, leidos) = (salida(0), salida(1));
            if escritos != leidos {
                unsafe { core::arch::asm!("dmb ishld") };
                let desde = VA_SALIDA + 64 + (leidos as usize % RANURAS) * PERIODO;
                for k in 0..CUADROS {
                    poner32(base + DATOS + 4 * k, unsafe { read_volatile((desde + 4 * k) as *const u32) });
                }
                poner_salida(1, leidos.wrapping_add(1));
                sonando = true;
            } else {
                for k in 0..CUADROS {
                    poner32(base + DATOS + 4 * k, 0);
                }
                if sonando {
                    vacios += 1;
                    poner_salida(4, vacios as u32);
                }
            }
            tx.ofrecer(cabeza, COLA_SALIDA);
            devueltos += 1;
            alguno = true;
            if devueltos % 1875 == 0 {
                println!("  [virtio-sonido] {} períodos, {} vacíos, {} errores", devueltos, vacios, errores);
            }
        }
        if alguno {
            dashos::canal_escribir(servicio, palabras(&[P_TIC]), &[]);
        }
    }
}

fn salida(i: usize) -> u32 {
    unsafe { read_volatile((VA_SALIDA + 4 * i) as *const u32) }
}

fn poner_salida(i: usize, v: u32) {
    unsafe { write_volatile((VA_SALIDA + 4 * i) as *mut u32, v) }
}

fn palabras(p: &[u32]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(p.as_ptr() as *const u8, 4 * p.len()) }
}
