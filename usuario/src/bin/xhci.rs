//! El controlador USB por xHCI (U6): el de los portátiles y placas con UEFI, el de QEMU `virt` y, en la Pi 4, el
//! VL805 (por el PCIe). Escrito con la especificación de xHCI 1.2 de Intel (registros de capacidad, de operación y de
//! puertos, cap. 5).
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 0); la 3, el canal de entrada del compositor.
//!
//! Reinicia el controlador (U6b), monta sus anillos de órdenes y de eventos, da dirección a lo que haya en cada puerto,
//! y a los teclados y ratones de arranque les configura su punto de interrupción y manda sus eventos al compositor,
//! como `usb.rs` (U6c). Sin interrupciones por ahora: mira los eventos en cada tic de un temporizador de 10 ms. Toda su
//! memoria es un objeto DMA del permiso, sin caché; su dirección de bus es la física (permiso de desplazamiento 0).

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x0090_0000;

// Capacidad (5.3).
const CAPLENGTH: usize = 0x00;
const HCIVERSION: usize = 0x02;
const HCSPARAMS1: usize = 0x04;
// Operación (5.4), desde CAPLENGTH.
const USBCMD: usize = 0x00;
const USBSTS: usize = 0x04;
const PORTSC: usize = 0x400; // + 0x10 · (puerto - 1)
const CMD_RUN: u32 = 1 << 0;
const CMD_HCRST: u32 = 1 << 1;
const STS_HCH: u32 = 1 << 0;
const STS_CNR: u32 = 1 << 11;
const PORT_CCS: u32 = 1 << 0;
const PORT_PED: u32 = 1 << 1;
const PORT_PR: u32 = 1 << 4;
const PORT_PRC: u32 = 1 << 21;
/// Bits de PORTSC que se borran escribiendo 1 (y PED, que apaga el puerto): fuera al escribir otra cosa.
const PORT_RW1C: u32 = 0x00FE_0002;
const HCCPARAMS1: usize = 0x10;
const DBOFF: usize = 0x14;
const RTSOFF: usize = 0x18;
const CRCR: usize = 0x18;
const DCBAAP: usize = 0x30;
const CONFIG: usize = 0x38;

// Memoria (un objeto DMA de 128 KB): dónde va cada cosa.
const VA_DMA: usize = 0x30_0000_0000;
const TAM_DMA: usize = 128 * 1024;
const M_DCBAA: usize = 0x0000;
const M_ERST: usize = 0x0400;
const M_ORDENES: usize = 0x1000;
const M_EVENTOS: usize = 0x2000;
const M_RANURAS: usize = 0x4000; // + 0x4000 por ranura
const TRBS: usize = 64;
const MAX_RANURAS: usize = 4;

// Tipos de TRB (6.4.6).
const T_NORMAL: u32 = 1;
const T_SETUP: u32 = 2;
const T_DATOS: u32 = 3;
const T_ESTADO: u32 = 4;
const T_ENLACE: u32 = 6;
const T_PEDIR_RANURA: u32 = 9;
const T_DIRECCION: u32 = 11;
const T_CONFIGURAR_EP: u32 = 12;
const T_EV_TRANSFERENCIA: u32 = 32;
const T_EV_ORDEN: u32 = 33;

fn leer64_dma(off: usize) -> u64 {
    u64::from_le(unsafe { core::ptr::read_volatile((VA_DMA + off) as *const u64) })
}

fn leer(off: usize) -> u32 {
    u32::from_le(unsafe { core::ptr::read_volatile((VA + off) as *const u32) })
}

fn escribir(off: usize, v: u32) {
    unsafe { core::ptr::write_volatile((VA + off) as *mut u32, v.to_le()) }
}

/// Espera a que se cumpla `cond`, como mucho `ms` milisegundos (girando: solo al arrancar).
fn esperar(ms: u64, cond: impl Fn() -> bool) -> bool {
    let fin = dashos::ahora_us() + ms * 1000;
    while dashos::ahora_us() < fin {
        if cond() {
            return true;
        }
    }
    cond()
}


/// Un anillo de TRB (órdenes o transferencias): 64 de 16 bytes, el último un enlace al primero que da la vuelta al
/// bit de ciclo (4.9.2).
#[derive(Clone, Copy)]
struct Anillo {
    off: usize,
    bus: u64,
    i: usize,
    ciclo: u32,
}

fn poner_trb(off: usize, parametro: u64, estado: u32, control: u32) {
    unsafe {
        core::ptr::write_volatile((VA_DMA + off) as *mut u64, parametro.to_le());
        core::ptr::write_volatile((VA_DMA + off + 8) as *mut u32, estado.to_le());
        core::arch::asm!("dmb oshst");
        core::ptr::write_volatile((VA_DMA + off + 12) as *mut u32, control.to_le()); // el ciclo, lo último
    }
}

impl Anillo {
    fn nuevo(off: usize, bus_base: u64) -> Self {
        let bus = bus_base + off as u64;
        // El enlace del final: al principio, con "dar la vuelta al ciclo" (bit 1).
        poner_trb(off + 16 * (TRBS - 1), bus, 0, (T_ENLACE << 10) | 2);
        Self { off, bus, i: 0, ciclo: 1 }
    }

    /// Pone un TRB y devuelve su dirección de bus.
    fn poner(&mut self, parametro: u64, estado: u32, control: u32) -> u64 {
        let donde = self.bus + 16 * self.i as u64;
        poner_trb(self.off + 16 * self.i, parametro, estado, control | self.ciclo);
        self.i += 1;
        if self.i == TRBS - 1 {
            // El enlace pasa a ser del ciclo actual, y el ciclo da la vuelta.
            poner_trb(self.off + 16 * (TRBS - 1), self.bus, 0, (T_ENLACE << 10) | 2 | self.ciclo);
            self.ciclo ^= 1;
            self.i = 0;
        }
        donde
    }
}

/// Un evento: (parámetro, estado, control).
type Evento = (u64, u32, u32);

struct Xhci {
    db: usize,
    rt: usize,
    bus: u64,
    ctx: usize, // 32 o 64 bytes por contexto
    ordenes: Anillo,
    ev_i: usize,
    ev_ciclo: u32,
}

impl Xhci {
    fn dma(&self, off: usize) -> *mut u32 {
        (VA_DMA + off) as *mut u32
    }

    fn poner32(&self, off: usize, v: u32) {
        unsafe { core::ptr::write_volatile(self.dma(off), v.to_le()) }
    }

    fn poner64(&self, off: usize, v: u64) {
        self.poner32(off, v as u32);
        self.poner32(off + 4, (v >> 32) as u32);
    }

    fn timbre(&self, n: usize, objetivo: u32) {
        unsafe { core::arch::asm!("dsb sy") };
        escribir(self.db + 4 * n, objetivo);
    }

    /// El siguiente evento del anillo, si lo hay; mueve el puntero de lectura (ERDP) y borra el "ocupado" (EHB).
    fn evento(&mut self) -> Option<Evento> {
        let off = M_EVENTOS + 16 * self.ev_i;
        let control = u32::from_le(unsafe { core::ptr::read_volatile(self.dma(off + 12)) });
        if control & 1 != self.ev_ciclo {
            return None;
        }
        let ev = (leer64_dma(off), u32::from_le(unsafe { core::ptr::read_volatile(self.dma(off + 8)) }), control);
        self.ev_i += 1;
        if self.ev_i == TRBS {
            self.ev_i = 0;
            self.ev_ciclo ^= 1;
        }
        let erdp = self.bus + (M_EVENTOS + 16 * self.ev_i) as u64;
        escribir(self.rt + 0x20 + 0x18, erdp as u32 | (1 << 3));
        escribir(self.rt + 0x20 + 0x1C, (erdp >> 32) as u32);
        Some(ev)
    }

    /// Espera un evento de ese tipo, como mucho `ms`; los demás se descartan (durante el arranque no hay otros que importen).
    fn esperar_evento(&mut self, tipo: u32, ms: u64) -> Option<Evento> {
        let fin = dashos::ahora_us() + ms * 1000;
        while dashos::ahora_us() < fin {
            if let Some(ev) = self.evento() {
                if (ev.2 >> 10) & 0x3f == tipo {
                    return Some(ev);
                }
            }
        }
        None
    }

    /// Una orden: devuelve (código de terminación, ranura).
    fn orden(&mut self, parametro: u64, control: u32) -> Option<(u32, u32)> {
        self.ordenes.poner(parametro, 0, control);
        self.timbre(0, 0);
        let ev = self.esperar_evento(T_EV_ORDEN, 500)?;
        Some((ev.1 >> 24, ev.2 >> 24))
    }
}

/// Lo de cada aparato con dirección.
#[derive(Clone, Copy)]
struct Ranura {
    id: u32,
    ep0: Anillo,
    int: Anillo,
    dci: u32,
    mps: u32,
    teclado: bool,
    activo: bool,
}

fn base_ranura(k: usize) -> usize {
    M_RANURAS + 0x4000 * k
}
// Dentro de cada ranura.
const R_SALIDA: usize = 0x0000;
const R_ENTRADA: usize = 0x1000;
const R_EP0: usize = 0x2000;
const R_INT: usize = 0x2400;
const R_DATOS: usize = 0x2800;
const R_INFORME: usize = 0x2C00;

impl Xhci {
    /// Una transferencia de control por el punto 0 (6.4.1.2): preparación, datos de entrada si `largo` > 0, y estado.
    fn control(&mut self, r: &mut Ranura, k: usize, setup: [u8; 8], largo: u32) -> Option<u32> {
        let tipo_transferencia = if largo > 0 { 3 } else { 0 }; // 3: datos de entrada
        r.ep0.poner(u64::from_le_bytes(setup), 8, (T_SETUP << 10) | (1 << 6) | (tipo_transferencia << 16));
        if largo > 0 {
            r.ep0.poner(self.bus + (base_ranura(k) + R_DATOS) as u64, largo, (T_DATOS << 10) | (1 << 16));
        }
        // Estado: al revés que los datos (de salida tras entrada; de entrada si no hubo datos), con aviso al acabar.
        r.ep0.poner(0, 0, (T_ESTADO << 10) | (1 << 5) | if largo > 0 { 0 } else { 1 << 16 });
        self.timbre(r.id as usize, 1);
        let ev = self.esperar_evento(T_EV_TRANSFERENCIA, 500)?;
        let codigo = ev.1 >> 24;
        (codigo == 1 || codigo == 13).then_some(largo.saturating_sub(ev.1 & 0xff_ffff))
    }

    fn datos(&self, k: usize, n: usize) -> [u8; 256] {
        let mut b = [0u8; 256];
        for (i, x) in b.iter_mut().enumerate().take(n.min(256)) {
            *x = unsafe { core::ptr::read_volatile((VA_DMA + base_ranura(k) + R_DATOS + i) as *const u8) };
        }
        b
    }
}

fn principal() -> u64 {
    // Lo primero, antes de crear llaves: la 3, si existe, es el canal de entrada del compositor.
    let copia = dashos::llave_duplicar(3, dashos::ESCRIBIR, 0);
    let canal = (copia > 0).then(|| {
        dashos::cerrar(copia as u32);
        3
    });
    if dashos::registros_proyectar(2, VA) != 0 {
        println!("  [xhci] sin registros");
        return 1;
    }
    let cap = leer(CAPLENGTH);
    let op = (cap & 0xff) as usize;
    let version = (leer(HCIVERSION & !3) >> 16) & 0xffff;
    let params = leer(HCSPARAMS1);
    let puertos = (params >> 24) as usize;
    let ranuras = ((params & 0xff) as usize).min(MAX_RANURAS);
    let ctx = if leer(HCCPARAMS1) & (1 << 2) != 0 { 64 } else { 32 };
    // Parar (por si el firmware lo dejó en marcha) y reiniciar (4.2).
    escribir(op + USBCMD, leer(op + USBCMD) & !CMD_RUN);
    if !esperar(20, || leer(op + USBSTS) & STS_HCH != 0) {
        println!("  [xhci] no se para");
        return 2;
    }
    escribir(op + USBCMD, CMD_HCRST);
    if !esperar(1000, || leer(op + USBCMD) & CMD_HCRST == 0 && leer(op + USBSTS) & STS_CNR == 0) {
        println!("  [xhci] no acaba de reiniciarse");
        return 3;
    }
    // Con el controlador reiniciado, lo que dejara en cuarentena uno anterior puede volver (M5).
    dashos::dma_liberar(1);
    let (m, bus) = dashos::memoria_dma(1, TAM_DMA);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_DMA, true) != 0 {
        println!("  [xhci] sin memoria DMA");
        return 4;
    }
    let mut x = Xhci { db: leer(DBOFF) as usize & !3, rt: leer(RTSOFF) as usize & !0x1f, bus, ctx, ordenes: Anillo::nuevo(M_ORDENES, bus), ev_i: 0, ev_ciclo: 1 };
    // Ranuras, tabla de contextos y anillo de órdenes (4.2, 5.4.5 a 5.4.7).
    escribir(op + CONFIG, ranuras as u32);
    escribir(op + DCBAAP, (bus + M_DCBAA as u64) as u32);
    escribir(op + DCBAAP + 4, ((bus + M_DCBAA as u64) >> 32) as u32);
    let crcr = x.ordenes.bus | 1;
    escribir(op + CRCR, crcr as u32);
    escribir(op + CRCR + 4, (crcr >> 32) as u32);
    // Anillo de eventos del interruptor 0: un segmento de 64 (5.5.2).
    x.poner64(M_ERST, bus + M_EVENTOS as u64);
    x.poner32(M_ERST + 8, TRBS as u32);
    let ir = x.rt + 0x20;
    escribir(ir + 0x08, 1);
    let erdp = bus + M_EVENTOS as u64;
    escribir(ir + 0x18, erdp as u32);
    escribir(ir + 0x1C, (erdp >> 32) as u32);
    let erst = bus + M_ERST as u64;
    escribir(ir + 0x10, erst as u32);
    escribir(ir + 0x14, (erst >> 32) as u32);
    escribir(op + USBCMD, CMD_RUN);
    if !esperar(100, || leer(op + USBSTS) & STS_HCH == 0) {
        println!("  [xhci] no arranca");
        return 5;
    }
    let mut lista: [Option<Ranura>; MAX_RANURAS] = [None; MAX_RANURAS];
    let mut n = 0;
    for p in 1..=puertos {
        let reg = op + PORTSC + 0x10 * (p - 1);
        if leer(reg) & PORT_CCS == 0 || n >= ranuras {
            continue;
        }
        // Reiniciar el puerto (los de USB 2; a los de USB 3 no les hace daño) y esperar a que acabe (4.3.1).
        escribir(reg, (leer(reg) & !PORT_RW1C) | PORT_PR);
        if !esperar(200, || leer(reg) & PORT_PRC != 0) || leer(reg) & PORT_PED == 0 {
            println!("  [xhci] puerto {}: no se activa", p);
            continue;
        }
        escribir(reg, (leer(reg) & !PORT_RW1C) | PORT_PRC);
        let velocidad_p = (leer(reg) >> 10) & 0xf;
        match configurar(&mut x, p as u32, velocidad_p, n) {
            Some(r) => {
                println!("  [xhci] puerto {}: ranura {}, {}", p, r.id, if r.dci == 0 { "sin teclado ni ratón de arranque" } else if r.teclado { "teclado" } else { "ratón" });
                lista[n] = Some(r);
                n += 1;
            }
            None => println!("  [xhci] puerto {}: no se pudo configurar", p),
        }
    }
    let teclados = lista.iter().flatten().filter(|r| r.activo && r.teclado).count();
    let ratones = lista.iter().flatten().filter(|r| r.activo && !r.teclado).count();
    println!("  [xhci] versión {:x}.{:02x}, {} puertos; HID de arranque: {} teclado y {} ratón", version >> 8, version & 0xff, puertos, teclados, ratones);
    // Cada 10 ms se miran los eventos: los informes que llegan van al compositor y se pide el siguiente.
    let mut entrada = dashos::hid::Entrada::nueva(canal, "xhci");
    let (a, b) = dashos::canal_crear();
    if dashos::temporizador_crear(b, 10_000, 10_000) <= 0 {
        return 6;
    }
    let mut buf = [0u8; 8];
    let mut ll = [0u32; 1];
    loop {
        dashos::canal_leer(a, &mut buf, &mut ll);
        while let Some(ev) = x.evento() {
            if (ev.2 >> 10) & 0x3f != T_EV_TRANSFERENCIA {
                continue;
            }
            let (ranura, dci, codigo) = (ev.2 >> 24, (ev.2 >> 16) & 0x1f, ev.1 >> 24);
            let Some(k) = lista.iter().position(|r| r.is_some_and(|r| r.id == ranura && r.dci == dci)) else { continue };
            let r = lista[k].as_mut().unwrap();
            if codigo == 1 || codigo == 13 {
                let largo = r.mps.saturating_sub(ev.1 & 0xff_ffff) as usize;
                let mut informe = [0u8; 16];
                for (i, b) in informe.iter_mut().enumerate().take(largo.min(16)) {
                    *b = unsafe { core::ptr::read_volatile((VA_DMA + base_ranura(k) + R_INFORME + i) as *const u8) };
                }
                if r.teclado { entrada.teclado(&informe[..largo.min(16)]) } else { entrada.raton(&informe[..largo.min(16)]) }
            }
            // El siguiente informe.
            r.int.poner(bus + (base_ranura(k) + R_INFORME) as u64, r.mps, (T_NORMAL << 10) | (1 << 5) | (1 << 2));
            x.timbre(r.id as usize, r.dci);
        }
    }
}

/// Pide una ranura, da dirección al aparato del puerto `p` y, si es un teclado o ratón de arranque, configura su punto
/// de interrupción y pide el primer informe (4.3.2 a 4.3.5). `k` es su sitio en la memoria.
fn configurar(x: &mut Xhci, p: u32, velocidad: u32, k: usize) -> Option<Ranura> {
    let (codigo, id) = x.orden(0, T_PEDIR_RANURA << 10)?;
    if codigo != 1 || id == 0 {
        return None;
    }
    let base = base_ranura(k);
    let bus = x.bus;
    // La tabla de contextos apunta a su contexto de salida.
    x.poner64(M_DCBAA + 8 * id as usize, bus + (base + R_SALIDA) as u64);
    let ep0 = Anillo::nuevo(base + R_EP0, bus);
    let int = Anillo::nuevo(base + R_INT, bus);
    let c = x.ctx;
    let mps0 = match velocidad {
        2 | 1 => 8,
        3 => 64,
        _ => 512,
    };
    // Contexto de entrada (6.2.5): control (añadir ranura y punto 0), ranura y punto 0.
    let e = base + R_ENTRADA;
    for i in (0..33 * c).step_by(4) {
        x.poner32(e + i, 0);
    }
    x.poner32(e + 4, 0b11);
    x.poner32(e + c, (velocidad << 20) | (1 << 27));
    x.poner32(e + c + 4, p << 16);
    x.poner32(e + 2 * c + 4, (3 << 1) | (4 << 3) | (mps0 << 16));
    x.poner64(e + 2 * c + 8, ep0.bus | 1);
    x.poner32(e + 2 * c + 16, 8);
    let (codigo, _) = x.orden(bus + e as u64, (T_DIRECCION << 10) | (id << 24))?;
    if codigo != 1 {
        return None;
    }
    let mut r = Ranura { id, ep0, int, dci: 0, mps: 0, teclado: false, activo: false };
    // Descriptor de aparato (18) y de configuración (9 y después entero).
    x.control(&mut r, k, [0x80, 6, 0, 1, 0, 0, 18, 0], 18)?;
    let n = x.control(&mut r, k, [0x80, 6, 0, 2, 0, 0, 9, 0], 9)?;
    let d = x.datos(k, n as usize);
    let total = u16::from_le_bytes([d[2], d[3]]).min(256) as u32;
    let valor = d[5];
    let n = x.control(&mut r, k, [0x80, 6, 0, 2, 0, 0, total as u8, (total >> 8) as u8], total)? as usize;
    let d = x.datos(k, n);
    // Interfaz HID de arranque (clase 3, subclase 1, protocolo 1 teclado o 2 ratón) y su punto de entrada de
    // interrupción.
    let (mut i, mut interfaz, mut hallado) = (0, None, None);
    while i + 1 < n && d[i] != 0 {
        match d[i + 1] {
            4 if i + 8 < n => {
                interfaz = (d[i + 5] == 3 && d[i + 6] == 1 && (d[i + 7] == 1 || d[i + 7] == 2)).then_some((d[i + 2], d[i + 7] == 1));
            }
            5 if i + 6 < n => {
                if let Some((ifz, teclado)) = interfaz {
                    if d[i + 2] & 0x80 != 0 && d[i + 3] & 3 == 3 && hallado.is_none() {
                        let mps = u16::from_le_bytes([d[i + 4], d[i + 5]]) as u32 & 0x7ff;
                        hallado = Some((ifz, teclado, (d[i + 2] & 0xf) as u32, mps, d[i + 6]));
                    }
                }
            }
            _ => {}
        }
        i += (d[i] as usize).max(2);
    }
    x.control(&mut r, k, [0x00, 9, valor, 0, 0, 0, 0, 0], 0)?; // SET_CONFIGURATION
    let Some((ifz, teclado, num, mps, b_intervalo)) = hallado else { return Some(r) };
    x.control(&mut r, k, [0x21, 0x0B, 0, 0, ifz, 0, 0, 0], 0)?; // SET_PROTOCOL: arranque
    let _ = x.control(&mut r, k, [0x21, 0x0A, 0, 0, ifz, 0, 0, 0], 0); // SET_IDLE 0: puede no admitirse
    // Configurar el punto (4.6.6): contexto de entrada con la ranura y el punto nuevo (DCI = 2 · número + 1).
    let dci = 2 * num + 1;
    for i in (0..33 * c).step_by(4) {
        x.poner32(e + i, 0);
    }
    x.poner32(e + 4, 1 | (1 << dci));
    x.poner32(e + c, (velocidad << 20) | (dci << 27));
    x.poner32(e + c + 4, p << 16);
    // Intervalo en potencias de dos de 125 µs (6.2.3.6): en alta velocidad, bInterval - 1; si no, bInterval ms.
    let intervalo = if velocidad >= 3 { (b_intervalo as u32).clamp(1, 16) - 1 } else { (31 - ((b_intervalo as u32).max(1) * 8).leading_zeros()).clamp(3, 10) };
    let ep = e + (dci as usize + 1) * c;
    x.poner32(ep, intervalo << 16);
    x.poner32(ep + 4, (3 << 1) | (7 << 3) | (mps << 16));
    x.poner64(ep + 8, r.int.bus | 1);
    x.poner32(ep + 16, mps | (mps << 16));
    let (codigo, _) = x.orden(bus + e as u64, (T_CONFIGURAR_EP << 10) | (id << 24))?;
    if codigo != 1 {
        return Some(r);
    }
    r.dci = dci;
    r.mps = mps;
    r.teclado = teclado;
    r.activo = true;
    // El primer informe.
    r.int.poner(bus + (base + R_INFORME) as u64, mps, (T_NORMAL << 10) | (1 << 5) | (1 << 2));
    x.timbre(id as usize, dci);
    Some(r)
}
