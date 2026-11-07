//! El controlador USB por xHCI (U6): el de los portátiles y placas con UEFI, el de QEMU `virt` y, en la Pi 4, el
//! VL805 (por el PCIe). Escrito con la especificación de xHCI 1.2 de Intel (registros de capacidad, de operación y de
//! puertos, cap. 5).
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 0); la 3, su interrupción (INTx), si el kernel la sabe;
//! la siguiente, el canal de entrada del compositor.
//!
//! Reinicia el controlador (U6b), monta sus anillos de órdenes y de eventos, da dirección a lo que haya en cada puerto,
//! y a los teclados y ratones de arranque les configura su punto de interrupción y manda sus eventos al compositor,
//! como `usb.rs` (U6c). Con su interrupción, despierta con ella (U6e); sin ella, mira los eventos en cada tic de un
//! temporizador de 10 ms. Toda su
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
const CMD_INTE: u32 = 1 << 2;
const STS_EINT: u32 = 1 << 3;
/// IMAN del interruptor (5.5.2.1): IP (se borra con 1) e IE.
const IMAN_IP: u32 = 1 << 0;
const IMAN_IE: u32 = 1 << 1;
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
const MAX_RANURAS: usize = 6;

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
    /// Es un concentrador (clase 9): sus puertos se recorren aparte (U6k).
    hub: bool,
    velocidad: u32,
    ruta: u32,
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
    // Lo primero, antes de crear llaves: si hay llave 4, la 3 es la interrupción y la 4 el canal de entrada del
    // compositor; si no, el canal (si lo hay) es la 3.
    let existe = |k: u32| {
        let copia = dashos::llave_duplicar(k, dashos::ESCRIBIR, 0);
        if copia > 0 {
            dashos::cerrar(copia as u32);
        }
        copia > 0
    };
    let (irq, canal) = if existe(4) { (Some(3), Some(4)) } else { (None, existe(3).then_some(3)) };
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
        match configurar(&mut x, p as u32, velocidad_p, n, 0, None) {
            Some(r) => {
                lista[n] = Some(r);
                n += 1;
                if r.hub {
                    println!("  [xhci] puerto {}: ranura {}, concentrador", p, r.id);
                    concentrador(&mut x, p as u32, n - 1, None, &mut lista, &mut n, ranuras);
                } else {
                    println!("  [xhci] puerto {}: ranura {}, {}", p, r.id, que_es(&r));
                }
            }
            None => println!("  [xhci] puerto {}: no se pudo configurar", p),
        }
    }
    let teclados = lista.iter().flatten().filter(|r| r.activo && r.teclado).count();
    let ratones = lista.iter().flatten().filter(|r| r.activo && !r.teclado).count();
    println!("  [xhci] versión {:x}.{:02x}, {} puertos; HID de arranque: {} teclado y {} ratón", version >> 8, version & 0xff, puertos, teclados, ratones);
    // Los eventos se miran al llegar la interrupción (o cada 10 ms sin ella): los informes que llegan van al
    // compositor y se pide el siguiente.
    let mut entrada = dashos::hid::Entrada::nueva(canal, "xhci");
    let (a, b) = dashos::canal_crear();
    let irq = irq.filter(|&i| dashos::interrupcion_asociar(i, b) == 0);
    if irq.is_none() && dashos::temporizador_crear(b, 10_000, 10_000) <= 0 {
        return 6;
    }
    println!("  [xhci] avisos {}", if irq.is_some() { "por su interrupción" } else { "cada 10 ms" });
    if irq.is_some() {
        escribir(ir, IMAN_IE | IMAN_IP);
        escribir(op + USBCMD, CMD_RUN | CMD_INTE);
    }
    let mut buf = [0u8; 8];
    let mut ll = [0u32; 1];
    let mut n_aviso = 0; // el tamaño del último aviso: 4, de la interrupción; 8, del temporizador
    loop {
        // Borrar el aviso antes de mirar los eventos: uno que llegue mientras vuelve a ponerlo (4.17.5).
        if irq.is_some() {
            escribir(op + USBSTS, STS_EINT);
            escribir(ir, IMAN_IE | IMAN_IP);
        }
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
        // La interrupción se vuelve a permitir con el aviso ya borrado; si llegó otro evento, salta otra vez.
        if let Some(i) = irq.filter(|_| n_aviso == 4) {
            dashos::interrupcion_terminar(i);
        }
        n_aviso = dashos::canal_leer(a, &mut buf, &mut ll).0;
    }
}

fn que_es(r: &Ranura) -> &'static str {
    if r.dci == 0 { "sin teclado ni ratón de arranque" } else if r.teclado { "teclado" } else { "ratón" }
}

/// Los saltos de concentrador de una cadena de ruta (cuántos grupos de 4 bits tiene).
fn saltos(ruta: u32) -> u32 {
    (0..5).take_while(|i| (ruta >> (4 * i)) & 0xf != 0).count() as u32
}

/// Recorre los puertos de un concentrador (USB 2.0, cap. 11.23 y 11.24): lo marca como concentrador en su ranura
/// (Hub y número de puertos, 6.2.2), enciende sus puertos, y a cada uno con algo lo reinicia y le da ranura, con la
/// ruta del concentrador más el puerto. Los concentradores de detrás, también (hasta 5 saltos). `k` es el sitio del
/// concentrador en `lista`; `tt`, el traductor que ya lleve.
fn concentrador(x: &mut Xhci, p: u32, k: usize, tt: Option<(u32, u32)>, lista: &mut [Option<Ranura>; MAX_RANURAS], n: &mut usize, max: usize) {
    let Some(mut h) = lista[k] else { return };
    // Descriptor de concentrador (tipo 0x29): puertos en el byte 2; tiempo hasta tener corriente en el 5 (2 ms).
    let Some(_) = x.control(&mut h, k, [0xA0, 6, 0, 0x29, 0, 0, 9, 0], 9) else { return };
    let d = x.datos(k, 9);
    let (puertos, espera) = (d[2] as u32, d[5] as u64 * 2);
    // La ranura como concentrador (con "configurar punto", solo el contexto de ranura).
    let c = x.ctx;
    let e = base_ranura(k) + R_ENTRADA;
    for i in (0..33 * c).step_by(4) {
        x.poner32(e + i, 0);
    }
    x.poner32(e + 4, 1);
    x.poner32(e + c, (h.velocidad << 20) | (1 << 26) | (1 << 27) | h.ruta);
    x.poner32(e + c + 4, p << 16 | puertos << 24);
    x.poner32(e + c + 8, tt.map_or(0, |(ranura, puerto)| ranura | puerto << 8));
    let _ = x.orden(x.bus + e as u64, (T_CONFIGURAR_EP << 10) | (h.id << 24));
    let salto = saltos(h.ruta);
    for i in 1..=puertos.min(15) {
        let _ = x.control(&mut h, k, [0x23, 3, 8, 0, i as u8, 0, 0, 0], 0); // SET_FEATURE(PORT_POWER)
    }
    dashos::dormir_us(espera.max(20) * 1000);
    for i in 1..=puertos.min(15) {
        if *n >= max {
            break;
        }
        // GET_STATUS del puerto: estado (bit 0 conectado, 1 activo, 9 baja velocidad, 10 alta) y cambios.
        let estado = |x: &mut Xhci, h: &mut Ranura| -> Option<(u16, u16)> {
            x.control(h, k, [0xA3, 0, 0, 0, i as u8, 0, 4, 0], 4)?;
            let d = x.datos(k, 4);
            Some((u16::from_le_bytes([d[0], d[1]]), u16::from_le_bytes([d[2], d[3]])))
        };
        let Some((e0, _)) = estado(x, &mut h) else { continue };
        if e0 & 1 == 0 {
            continue;
        }
        let _ = x.control(&mut h, k, [0x23, 3, 4, 0, i as u8, 0, 0, 0], 0); // SET_FEATURE(PORT_RESET)
        let mut listo = None;
        for _ in 0..20 {
            dashos::dormir_us(10_000);
            if let Some((e1, cambio)) = estado(x, &mut h) {
                if cambio & (1 << 4) != 0 && e1 & 2 != 0 {
                    listo = Some(e1);
                    break;
                }
            }
        }
        let _ = x.control(&mut h, k, [0x23, 1, 20, 0, i as u8, 0, 0, 0], 0); // CLEAR_FEATURE(C_PORT_RESET)
        let _ = x.control(&mut h, k, [0x23, 1, 16, 0, i as u8, 0, 0, 0], 0); // CLEAR_FEATURE(C_PORT_CONNECTION)
        let Some(e1) = listo else {
            println!("  [xhci] concentrador {}, puerto {}: no se activa", h.id, i);
            continue;
        };
        // Velocidades de xHCI (7.2.2.1.1): plena 1, baja 2, alta 3.
        let velocidad = if e1 & (1 << 9) != 0 { 2 } else if e1 & (1 << 10) != 0 { 3 } else { 1 };
        // Un aparato lento detrás de un concentrador de alta velocidad va por su traductor.
        let tt_hijo = if h.velocidad == 3 && velocidad < 3 { Some((h.id, i)) } else { tt };
        let ruta = h.ruta | i << (4 * salto);
        match configurar(x, p, velocidad, *n, ruta, tt_hijo) {
            Some(r) => {
                lista[*n] = Some(r);
                *n += 1;
                if r.hub && salto < 4 {
                    println!("  [xhci] puerto {} ruta {:x}: ranura {}, concentrador", p, ruta, r.id);
                    concentrador(x, p, *n - 1, tt_hijo, lista, n, max);
                } else {
                    println!("  [xhci] puerto {} ruta {:x}: ranura {}, {}", p, ruta, r.id, que_es(&r));
                }
            }
            None => println!("  [xhci] puerto {} ruta {:x}: no se pudo configurar", p, ruta),
        }
    }
    lista[k] = Some(h);
}

/// Pide una ranura, da dirección al aparato del puerto `p` y, si es un teclado o ratón de arranque, configura su punto
/// de interrupción y pide el primer informe (4.3.2 a 4.3.5). `k` es su sitio en la memoria.
/// `ruta` es la cadena de ruta (4 bits por salto de concentrador, 4.3.3; 0 en un puerto raíz) y `tt`, el traductor
/// de un aparato de baja o plena velocidad detrás de un concentrador de alta (ranura y puerto del concentrador).
fn configurar(x: &mut Xhci, p: u32, velocidad: u32, k: usize, ruta: u32, tt: Option<(u32, u32)>) -> Option<Ranura> {
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
    let tt = tt.map_or(0, |(ranura, puerto)| ranura | puerto << 8);
    x.poner32(e + c, (velocidad << 20) | (1 << 27) | ruta);
    x.poner32(e + c + 4, p << 16);
    x.poner32(e + c + 8, tt);
    x.poner32(e + 2 * c + 4, (3 << 1) | (4 << 3) | (mps0 << 16));
    x.poner64(e + 2 * c + 8, ep0.bus | 1);
    x.poner32(e + 2 * c + 16, 8);
    let (codigo, _) = x.orden(bus + e as u64, (T_DIRECCION << 10) | (id << 24))?;
    if codigo != 1 {
        return None;
    }
    let mut r = Ranura { id, hub: false, velocidad, ruta, ep0, int, dci: 0, mps: 0, teclado: false, activo: false };
    // Descriptor de aparato (18) y de configuración (9 y después entero).
    x.control(&mut r, k, [0x80, 6, 0, 1, 0, 0, 18, 0], 18)?;
    r.hub = x.datos(k, 18)[4] == 9;
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
    if r.hub {
        return Some(r);
    }
    let Some((ifz, teclado, num, mps, b_intervalo)) = hallado else { return Some(r) };
    x.control(&mut r, k, [0x21, 0x0B, 0, 0, ifz, 0, 0, 0], 0)?; // SET_PROTOCOL: arranque
    let _ = x.control(&mut r, k, [0x21, 0x0A, 0, 0, ifz, 0, 0, 0], 0); // SET_IDLE 0: puede no admitirse
    // Configurar el punto (4.6.6): contexto de entrada con la ranura y el punto nuevo (DCI = 2 · número + 1).
    let dci = 2 * num + 1;
    for i in (0..33 * c).step_by(4) {
        x.poner32(e + i, 0);
    }
    x.poner32(e + 4, 1 | (1 << dci));
    x.poner32(e + c, (velocidad << 20) | (dci << 27) | ruta);
    x.poner32(e + c + 4, p << 16);
    x.poner32(e + c + 8, tt);
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
