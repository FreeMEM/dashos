//! El controlador de sonido Intel High Definition Audio (X5c), fuera del kernel. Escrito con la Intel High Definition
//! Audio Specification 1.0a: registros del controlador (3.3), CORB y RIRB (4.4.1), descriptores de flujo y su lista de
//! búferes (3.3.35 a 3.3.44, 3.6.2), formato de flujo (3.7.1), y los verbos y parámetros del códec (7.3).
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 0); la 3, el permiso de la clase de plazo (S3); la 4, su
//! interrupción (MSI), si la hay; la siguiente, el canal "sonido" (el servicio, A4).
//!
//! Reinicia el controlador, monta el CORB y el RIRB para hablar con los códecs, busca en el primero que valga una
//! salida: una patilla de salida (línea, altavoz o auriculares, conectada) y un convertidor de salida (DAC) al que se
//! llegue desde ella, quizá por mezcladores o selectores. Enciende el camino, quita el silencio a los amplificadores
//! (a 0 dB), pone el DAC en estéreo de 16 bits a 48 kHz y lo une al flujo 1. Las demás patillas de salida que lleguen
//! al mismo DAC (auriculares y altavoz) se encienden también. Después suena el primer flujo de salida del controlador
//! con 8 búferes de 256 cuadros, uno por período, con una interrupción al acabar cada uno, y los llena con lo que mezcla
//! el servicio "sonido", como `virtio-sonido`: una memoria con ranuras de un período y un tic por cada búfer suelto.

#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x0090_0000;
const VA_DMA: usize = 0x30_0000_0000;
const TAM_DMA: usize = 64 * 1024;

// Registros (3.3).
const GCAP: usize = 0x00;
const VMIN: usize = 0x02;
const VMAJ: usize = 0x03;
const GCTL: usize = 0x08;
const STATESTS: usize = 0x0E;
const INTCTL: usize = 0x20;
const CORBLBASE: usize = 0x40;
const CORBUBASE: usize = 0x44;
const CORBWP: usize = 0x48;
const CORBRP: usize = 0x4A;
const CORBCTL: usize = 0x4C;
const CORBSIZE: usize = 0x4E;
const RIRBLBASE: usize = 0x50;
const RIRBUBASE: usize = 0x54;
const RIRBWP: usize = 0x58;
const RINTCNT: usize = 0x5A;
const RIRBCTL: usize = 0x5C;
const RIRBSTS: usize = 0x5D;
const RIRBSIZE: usize = 0x5E;
/// Los descriptores de flujo: desde 0x80, 0x20 cada uno; primero los de entrada, luego los de salida.
const SD: usize = 0x80;
const SD_CTL: usize = 0x00;
const SD_STS: usize = 0x03;
const SD_LPIB: usize = 0x04;
const SD_CBL: usize = 0x08;
const SD_LVI: usize = 0x0C;
const SD_FMT: usize = 0x12;
const SD_BDPL: usize = 0x18;
const SD_BDPU: usize = 0x1C;
const CTL_SRST: u32 = 1 << 0;
const CTL_RUN: u32 = 1 << 1;
const CTL_IOCE: u32 = 1 << 2;
const STS_BCIS: u8 = 1 << 2;

// La memoria DMA: CORB (256 entradas de 4 bytes), RIRB (256 de 8), la lista de búferes y los búferes.
const M_CORB: usize = 0x0000;
const M_RIRB: usize = 0x0800;
const M_BDL: usize = 0x1000;
const M_BUFERES: usize = 0x2000;

/// El flujo: estéreo de 16 bits a 48 kHz (3.7.1: base de 48 kHz, ×1 ÷1, 16 bits, 2 canales).
const FORMATO: u16 = 0x0011;
const FLUJO: u32 = 1;
const CUADROS: usize = 256;
const PERIODO: usize = CUADROS * 4;
const BUFERES: usize = 8;
const RANURAS: usize = 8;
const VA_SALIDA: usize = 0x40_0000_0000;
const P_SALIDA: u32 = 1;
const P_TIC: u32 = 2;

// Tipos de widget (7.3.4.6) y parámetros (7.3.4).
const W_SALIDA: u32 = 0x0;
const W_MEZCLADOR: u32 = 0x2;
const W_SELECTOR: u32 = 0x3;
const W_PATILLA: u32 = 0x4;
const PAR_NODOS: u32 = 0x04;
const PAR_GRUPO: u32 = 0x05;
const PAR_WIDGET: u32 = 0x09;
const PAR_PATILLA: u32 = 0x0C;
const PAR_AMP_ENTRADA: u32 = 0x0D;
const PAR_CONEXIONES: u32 = 0x0E;
const PAR_AMP_SALIDA: u32 = 0x12;
const MAX_CAMINO: usize = 5;

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
fn dma32(off: usize) -> u32 {
    u32::from_le(unsafe { read_volatile((VA_DMA + off) as *const u32) })
}
fn poner32(off: usize, v: u32) {
    unsafe { write_volatile((VA_DMA + off) as *mut u32, v.to_le()) }
}

fn esperar(ms: u64, cond: impl Fn() -> bool) -> bool {
    let fin = dashos::ahora_us() + ms * 1000;
    while dashos::ahora_us() < fin {
        if cond() {
            return true;
        }
    }
    cond()
}

/// El CORB y el RIRB: por dónde va cada uno.
struct Ordenes {
    corb: usize,
    rirb: usize,
    wp: usize,
    rp: usize,
}

impl Ordenes {
    /// Manda un verbo al nodo `nid` del códec `cad` (7.1.2) y espera su respuesta (las no pedidas se saltan).
    fn verbo(&mut self, cad: u32, nid: u32, verbo: u32) -> Option<u32> {
        self.wp = (self.wp + 1) % self.corb;
        poner32(M_CORB + 4 * self.wp, cad << 28 | (nid & 0xff) << 20 | (verbo & 0xf_ffff));
        dashos::barrera::total();
        poner_reg16(CORBWP, self.wp as u16);
        let fin = dashos::ahora_us() + 20_000;
        loop {
            if (reg16(RIRBWP) & 0xff) as usize != self.rp {
                self.rp = (self.rp + 1) % self.rirb;
                dashos::barrera::lecturas();
                let (r, ext) = (dma32(M_RIRB + 8 * self.rp), dma32(M_RIRB + 8 * self.rp + 4));
                poner_reg8(RIRBSTS, 0x05);
                if ext & 0x10 == 0 {
                    return Some(r);
                }
                continue;
            }
            if dashos::ahora_us() > fin {
                return None;
            }
        }
    }

    fn parametro(&mut self, cad: u32, nid: u32, p: u32) -> u32 {
        self.verbo(cad, nid, 0xF00 << 8 | p).unwrap_or(0)
    }

    /// La lista de conexiones de `nid` (7.3.3.3), con los rangos abiertos; cuántas hay.
    fn conexiones(&mut self, cad: u32, nid: u32, lista: &mut [u32; 32]) -> usize {
        let largo = self.parametro(cad, nid, PAR_CONEXIONES);
        let (n, larga) = ((largo & 0x7f) as usize, largo & 0x80 != 0);
        let (por, bits) = if larga { (2, 16) } else { (4, 8) };
        let mut k = 0;
        let mut i = 0;
        while i < n && k < lista.len() {
            let r = self.verbo(cad, nid, 0xF02 << 8 | i as u32).unwrap_or(0);
            for j in 0..por {
                if i + j >= n || k >= lista.len() {
                    break;
                }
                let e = (r >> (bits * j)) & ((1 << bits) - 1);
                let (nodo, rango) = (e & ((1 << (bits - 1)) - 1), e >> (bits - 1) != 0);
                if rango && k > 0 {
                    let mut x = lista[k - 1] + 1;
                    while x <= nodo && k < lista.len() {
                        lista[k] = x;
                        k += 1;
                        x += 1;
                    }
                } else {
                    lista[k] = nodo;
                    k += 1;
                }
            }
            i += por;
        }
        k
    }

    fn tipo(&mut self, cad: u32, nid: u32) -> u32 {
        (self.parametro(cad, nid, PAR_WIDGET) >> 20) & 0xf
    }

    /// Un camino de la patilla `nid` a un DAC (y, si se da, a ese DAC), por mezcladores y selectores: los nodos y,
    /// para cada uno menos el último, la conexión que lleva al siguiente.
    fn camino(&mut self, cad: u32, nid: u32, dac: Option<u32>, prof: usize, c: &mut [(u32, usize); MAX_CAMINO]) -> Option<usize> {
        if prof >= MAX_CAMINO {
            return None;
        }
        let t = self.tipo(cad, nid);
        c[prof] = (nid, 0);
        if t == W_SALIDA {
            return (dac.is_none() || dac == Some(nid)).then_some(prof + 1);
        }
        if prof > 0 && t != W_MEZCLADOR && t != W_SELECTOR {
            return None;
        }
        let mut lista = [0u32; 32];
        let n = self.conexiones(cad, nid, &mut lista);
        for (i, &s) in lista[..n].iter().enumerate() {
            c[prof] = (nid, i);
            if let Some(l) = self.camino(cad, s, dac, prof + 1, c) {
                return Some(l);
            }
        }
        None
    }

    /// Las capacidades del amplificador (de salida o de entrada) de `nid`: las suyas o, si no las tiene propias, las
    /// del grupo `afg` (7.3.4.10).
    fn amp(&mut self, cad: u32, afg: u32, nid: u32, salida: bool) -> u32 {
        let p = if salida { PAR_AMP_SALIDA } else { PAR_AMP_ENTRADA };
        let w = self.parametro(cad, nid, PAR_WIDGET);
        if w & (1 << 3) != 0 { self.parametro(cad, nid, p) } else { self.parametro(cad, afg, p) }
    }

    /// Enciende el camino (de la patilla al DAC): D0, la conexión elegida, los amplificadores sin silencio y a 0 dB,
    /// y la patilla con la salida encendida (y EAPD).
    fn encender(&mut self, cad: u32, afg: u32, c: &[(u32, usize)]) {
        for (k, &(nid, i)) in c.iter().enumerate() {
            let w = self.parametro(cad, nid, PAR_WIDGET);
            let t = (w >> 20) & 0xf;
            self.verbo(cad, nid, 0x705 << 8);
            let ultimo = k + 1 == c.len();
            if !ultimo && (t == W_SELECTOR || t == W_PATILLA) {
                self.verbo(cad, nid, 0x701 << 8 | i as u32);
            }
            if !ultimo && t == W_MEZCLADOR && w & (1 << 1) != 0 {
                let a = self.amp(cad, afg, nid, false);
                self.verbo(cad, nid, 0x3 << 16 | 0x7000 | (i as u32) << 8 | (a & 0x7f));
            }
            if w & (1 << 2) != 0 {
                let a = self.amp(cad, afg, nid, true);
                self.verbo(cad, nid, 0x3 << 16 | 0xB000 | (a & 0x7f));
            }
            if t == W_PATILLA {
                let pc = self.parametro(cad, nid, PAR_PATILLA);
                let hp = self.verbo(cad, nid, 0xF1C << 8).is_some_and(|d| (d >> 20) & 0xf == 2) && pc & (1 << 3) != 0;
                self.verbo(cad, nid, 0x707 << 8 | 0x40 | if hp { 0x80 } else { 0 });
                if pc & (1 << 16) != 0 {
                    self.verbo(cad, nid, 0x70C << 8 | 0x02);
                }
            }
        }
    }
}

/// Una patilla de salida que se puede usar (7.3.3.31): con salida, conectada (no "sin conexión física") y de línea,
/// altavoz o auriculares. Su preferencia (menos es mejor).
fn patilla_de_salida(o: &mut Ordenes, cad: u32, nid: u32) -> Option<u32> {
    if o.tipo(cad, nid) != W_PATILLA || o.parametro(cad, nid, PAR_PATILLA) & (1 << 4) == 0 {
        return None;
    }
    let d = o.verbo(cad, nid, 0xF1C << 8)?;
    let (conexion, aparato) = (d >> 30, (d >> 20) & 0xf);
    (conexion != 1 && aparato <= 2).then_some(aparato)
}

/// Busca en el códec `cad` su grupo de audio y una salida: (grupo, DAC, nombre de la patilla elegida, patillas
/// encendidas).
fn preparar_codec(o: &mut Ordenes, cad: u32) -> Option<(u32, u32, &'static str, usize)> {
    let raiz = o.parametro(cad, 0, PAR_NODOS);
    let (desde, n) = ((raiz >> 16) & 0xff, raiz & 0xff);
    let afg = (desde..desde + n).find(|&g| o.parametro(cad, g, PAR_GRUPO) & 0xff == 1)?;
    o.verbo(cad, afg, 0x705 << 8);
    let nodos = o.parametro(cad, afg, PAR_NODOS);
    let (desde, n) = ((nodos >> 16) & 0xff, nodos & 0xff);
    // La patilla preferida que llega a un DAC: línea (0), altavoz (1), auriculares (2).
    let mut mejor: Option<(u32, u32, [(u32, usize); MAX_CAMINO], usize)> = None;
    for nid in desde..desde + n {
        let Some(pref) = patilla_de_salida(o, cad, nid) else { continue };
        if mejor.is_some_and(|m| m.0 <= pref) {
            continue;
        }
        let mut c = [(0, 0); MAX_CAMINO];
        if let Some(l) = o.camino(cad, nid, None, 0, &mut c) {
            mejor = Some((pref, nid, c, l));
        }
    }
    let (pref, pin, c, l) = mejor?;
    let dac = c[l - 1].0;
    o.encender(cad, afg, &c[..l]);
    // Las demás patillas que lleguen al mismo DAC.
    let mut patillas = 1;
    for nid in desde..desde + n {
        if nid == pin || patilla_de_salida(o, cad, nid).is_none() {
            continue;
        }
        let mut c = [(0, 0); MAX_CAMINO];
        if let Some(l) = o.camino(cad, nid, Some(dac), 0, &mut c) {
            o.encender(cad, afg, &c[..l]);
            patillas += 1;
        }
    }
    // El DAC: formato y flujo 1, canal 0 (7.3.3.8 y 7.3.3.11).
    o.verbo(cad, dac, 0x2 << 16 | FORMATO as u32);
    o.verbo(cad, dac, 0x706 << 8 | FLUJO << 4);
    let nombre = ["línea", "altavoz", "auriculares"][pref as usize];
    Some((afg, dac, nombre, patillas))
}

fn principal() -> u64 {
    // Lo primero, antes de crear llaves: con llave 5, la 4 es la interrupción y la 5 el canal; si no, el canal es la 4.
    let existe = |k: u32| {
        let copia = dashos::llave_duplicar(k, dashos::LEER, 0);
        if copia > 0 {
            dashos::cerrar(copia as u32);
        }
        copia > 0
    };
    let (irq, servicio) = if existe(5) { (Some(4u32), 5u32) } else { (None, 4u32) };
    if dashos::registros_proyectar(2, VA) != 0 {
        println!("  [hda] sin registros");
        return 1;
    }
    let gcap = reg16(GCAP) as usize;
    let (entradas, salidas) = ((gcap >> 8) & 0xf, (gcap >> 12) & 0xf);
    if salidas == 0 {
        println!("  [hda] sin flujos de salida");
        return 2;
    }
    // Parar lo que dejara el firmware (los flujos, el CORB y el RIRB) y reiniciar el controlador (4.2.2): CRST a 0,
    // esperar, a 1, y dar a los códecs su tiempo para presentarse (521 µs; 4.3).
    for s in 0..(entradas + salidas) {
        let sd = SD + 0x20 * s;
        poner_reg32(sd + SD_CTL, reg32(sd + SD_CTL) & 0xff_fffc);
    }
    poner_reg8(CORBCTL, 0);
    poner_reg8(RIRBCTL, 0);
    esperar(10, || reg8(CORBCTL) & 2 == 0 && reg8(RIRBCTL) & 2 == 0);
    poner_reg32(GCTL, reg32(GCTL) & !1);
    if !esperar(100, || reg32(GCTL) & 1 == 0) {
        println!("  [hda] no entra en reinicio");
        return 3;
    }
    esperar(1, || false);
    poner_reg32(GCTL, reg32(GCTL) | 1);
    if !esperar(100, || reg32(GCTL) & 1 != 0) {
        println!("  [hda] no sale del reinicio");
        return 4;
    }
    esperar(2, || false);
    let codecs = reg16(STATESTS) & 0x7fff;
    poner_reg16(STATESTS, codecs);
    println!("  [hda] versión {}.{}, {} flujos de entrada y {} de salida, códecs {:#x}", reg8(VMAJ), reg8(VMIN), entradas, salidas, codecs);
    dashos::dma_liberar(1);
    let (m, bus) = dashos::memoria_dma(1, TAM_DMA);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_DMA, true) != 0 {
        println!("  [hda] sin memoria DMA");
        return 5;
    }
    for i in (0..TAM_DMA).step_by(4) {
        poner32(i, 0);
    }
    // El CORB y el RIRB, del tamaño más grande que admitan (4.4.1.3).
    let tamano = |r: usize| match reg8(r) >> 4 {
        c if c & 4 != 0 => (2u8, 256),
        c if c & 2 != 0 => (1, 16),
        _ => (0, 2),
    };
    let (cs, corb) = tamano(CORBSIZE);
    let (rs, rirb) = tamano(RIRBSIZE);
    poner_reg8(CORBSIZE, (reg8(CORBSIZE) & !3) | cs);
    poner_reg8(RIRBSIZE, (reg8(RIRBSIZE) & !3) | rs);
    poner_reg32(CORBLBASE, (bus + M_CORB as u64) as u32);
    poner_reg32(CORBUBASE, ((bus + M_CORB as u64) >> 32) as u32);
    poner_reg32(RIRBLBASE, (bus + M_RIRB as u64) as u32);
    poner_reg32(RIRBUBASE, ((bus + M_RIRB as u64) >> 32) as u32);
    // El puntero de lectura del CORB a 0: el bit de reinicio a 1 y otra vez a 0 (hay controladores que no lo dejan
    // leer a 1; se espera poco).
    poner_reg16(CORBRP, 1 << 15);
    esperar(1, || reg16(CORBRP) & (1 << 15) != 0);
    poner_reg16(CORBRP, 0);
    esperar(10, || reg16(CORBRP) & (1 << 15) == 0);
    poner_reg16(CORBWP, 0);
    poner_reg16(RIRBWP, 1 << 15);
    // Con RINTCTL (bit 0): al llegar RINTCNT respuestas se pone RIRBSTS.RINTFL, que se borra tras cada una. Sin él,
    // hay controladores (el de QEMU) que paran el CORB al llegar a la cuenta. No llega a interrumpir: INTCTL.CIE va
    // apagado.
    poner_reg16(RINTCNT, 1);
    poner_reg8(RIRBCTL, 3);
    poner_reg8(CORBCTL, 2);
    let mut o = Ordenes { corb, rirb, wp: 0, rp: 0 };
    // El primer códec con una salida.
    let mut elegido = None;
    for cad in 0..15u32 {
        if codecs & (1 << cad) == 0 {
            continue;
        }
        let fab = o.parametro(cad, 0, 0x00);
        match preparar_codec(&mut o, cad) {
            Some((_, dac, patilla, n)) => {
                println!("  [hda] códec {} ({:04x}:{:04x}): salida por el DAC {} y la patilla de {} ({} patillas)", cad, fab >> 16, fab & 0xffff, dac, patilla, n);
                elegido = Some(cad);
                break;
            }
            None => println!("  [hda] códec {} ({:04x}:{:04x}): sin salida que usar", cad, fab >> 16, fab & 0xffff),
        }
    }
    if elegido.is_none() {
        println!("  [hda] ningún códec con salida");
        return 6;
    }
    // El primer flujo de salida (3.3.35): reiniciarlo, su lista de búferes (cada uno con su interrupción), su formato
    // y su número.
    let s = entradas;
    let sd = SD + 0x20 * s;
    poner_reg32(sd + SD_CTL, CTL_SRST);
    esperar(10, || reg32(sd + SD_CTL) & CTL_SRST != 0);
    poner_reg32(sd + SD_CTL, 0);
    esperar(10, || reg32(sd + SD_CTL) & CTL_SRST == 0);
    for b in 0..BUFERES {
        let e = M_BDL + 16 * b;
        let datos = bus + (M_BUFERES + b * PERIODO) as u64;
        poner32(e, datos as u32);
        poner32(e + 4, (datos >> 32) as u32);
        poner32(e + 8, PERIODO as u32);
        poner32(e + 12, 1);
    }
    poner_reg32(sd + SD_CBL, (BUFERES * PERIODO) as u32);
    poner_reg16(sd + SD_LVI, (BUFERES - 1) as u16);
    poner_reg16(sd + SD_FMT, FORMATO);
    poner_reg32(sd + SD_BDPL, (bus + M_BDL as u64) as u32);
    poner_reg32(sd + SD_BDPU, ((bus + M_BDL as u64) >> 32) as u32);
    poner_reg8(sd + SD_STS, 0x1c);
    let avisos = irq.and_then(|i| {
        let (lee, escribe) = dashos::canal_crear();
        (dashos::interrupcion_asociar(i, escribe) == 0).then_some((lee, i))
    });
    if avisos.is_some() {
        poner_reg32(INTCTL, 1 << 31 | 1 << s);
    }
    // La salida que llena el servicio "sonido" (A4), como en `virtio-sonido`.
    let mem = dashos::memoria_crear(64 + RANURAS * PERIODO);
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_SALIDA, true) != 0 {
        println!("  [hda] sin memoria para la salida");
        return 7;
    }
    poner_salida(2, RANURAS as u32);
    poner_salida(3, CUADROS as u32);
    let copia = dashos::llave_duplicar(mem, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let peticion: [u32; 4] = [P_SALIDA, RANURAS as u32, CUADROS as u32, 48_000];
    if copia <= 0 || dashos::canal_escribir(servicio, palabras(&peticion), &[copia as u32]) != 0 {
        println!("  [hda] el servicio no recibe la salida");
        return 8;
    }
    // A correr: flujo 1 en los bits 23:20, con interrupción al acabar cada búfer.
    poner_reg32(sd + SD_CTL, FLUJO << 20 | CTL_IOCE | CTL_RUN);
    println!("  [hda] suena el flujo de salida {} para el servicio", s);
    if dashos::plazo(3, 1_000, 5_333) != 0 {
        println!("  [hda] sin clase de plazo: va en la justa");
    }
    println!("  [hda] avisos {}", if avisos.is_some() { "por su interrupción" } else { "con un temporizador" });
    // Cada búfer que el controlador deja atrás (su posición, LPIB, ya está en el siguiente) se llena con el siguiente
    // período del servicio, o con silencio si no lo tiene (un vacío), y se avisa al servicio con un tic.
    let (mut sueltos, mut vacios) = (0u64, 0u64);
    let mut siguiente = 0usize;
    let mut sonando = false;
    loop {
        match avisos {
            Some((lee, i)) => {
                let mut m8 = [0u8; 8];
                let mut ll = [0u32; 1];
                dashos::canal_leer(lee, &mut m8, &mut ll);
                poner_reg8(sd + SD_STS, STS_BCIS);
                dashos::interrupcion_terminar(i);
            }
            None => {
                dashos::dormir_us(2000);
                poner_reg8(sd + SD_STS, STS_BCIS);
            }
        }
        let actual = (reg32(sd + SD_LPIB) as usize / PERIODO) % BUFERES;
        let mut alguno = false;
        let mut pedido = false;
        while siguiente != actual {
            let base = M_BUFERES + siguiente * PERIODO;
            let (mut escritos, leidos) = (salida(0), salida(1));
            // Tras un parón (el arranque, con la IOMMU, deja este proceso sin correr hasta 30 ms) hay más búferes que
            // llenar que períodos tiene el servicio por delante (4): se le avisa con un tic y se le espera hasta 2 ms
            // antes de poner silencio.
            if escritos == leidos && sonando && !pedido {
                pedido = true;
                dashos::canal_escribir(servicio, palabras(&[P_TIC]), &[]);
                let fin = dashos::ahora_us() + 2000;
                while salida(0) == leidos && dashos::ahora_us() < fin {
                    dashos::dormir_us(100);
                }
                escritos = salida(0);
            }
            if escritos != leidos {
                dashos::barrera::lecturas();
                let desde = VA_SALIDA + 64 + (leidos as usize % RANURAS) * PERIODO;
                for k in 0..CUADROS {
                    poner32(base + 4 * k, unsafe { read_volatile((desde + 4 * k) as *const u32) });
                }
                poner_salida(1, leidos.wrapping_add(1));
                sonando = true;
            } else {
                for k in 0..CUADROS {
                    poner32(base + 4 * k, 0);
                }
                if sonando {
                    vacios += 1;
                    poner_salida(4, vacios as u32);
                }
            }
            siguiente = (siguiente + 1) % BUFERES;
            sueltos += 1;
            alguno = true;
            if sueltos % 1875 == 0 {
                println!("  [hda] {} períodos, {} vacíos", sueltos, vacios);
            }
        }
        if alguno {
            dashos::barrera::escrituras();
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
