// El controlador USB por xHCI (U6): el de los portátiles y placas con UEFI, el de QEMU `virt`, en la Pi 4 el VL805
// (por el PCIe) y en la Pi 5 los dos del RP1 (DWC3 de Synopsys en modo anfitrión, que son xHCI normales; "RP1
// Peripherals", cap. 5). Escrito con la especificación de xHCI 1.2 de Intel (registros de capacidad, de operación y de
// puertos, cap. 5).
//
// Lo comparten tres programas (`src/bin/xhci.rs`, `xhci0.rs`, `xhci1.rs`) que solo cambian el nombre con que se
// presentan en el registro (`NOMBRE`): en la Pi 5 hay dos controladores y cada foto tiene que decir de cuál es cada
// línea. Sin `//!`: se incluye con `include!`.
//
// Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 0); la 3, su interrupción (INTx), si el kernel la sabe;
// la siguiente, el canal de entrada del compositor ("ventanas"); y, si la hay, "usb-dominio": el canal por el que un
// dominio de controladores (S35) pide aparatos prestados (`protocolo::usb_prestado`, `docs/bitacoras/dominio-usb.md`).
//
// Prestar (S35, paso 2): los aparatos que no usa él (ni teclados, ni ratones, ni concentradores) los ofrece al
// dominio que los pida. Al prestado le hace las transferencias que pida el dominio (de control, masivas y de
// interrupción, varias a la vez y cancelables), con los datos en unos búferes que presta con él. Lo de la pila USB lo
// sigue haciendo él: la dirección, configurar los puntos al cambiar de configuración o de alternativa, desatascar un
// punto. Al devolverlo (o si el dominio se va) desactiva su ranura, reinicia el puerto y lo vuelve a configurar como
// nuevo. Si se desenchufa (o se enchufa algo en un puerto raíz), lo mira en marcha.
//
// Reinicia el controlador (U6b), monta sus anillos de órdenes y de eventos, da dirección a lo que haya en cada puerto,
// y a los teclados y ratones de arranque les configura su punto de interrupción y manda sus eventos al compositor,
// como `usb.rs` (U6c). Con su interrupción, despierta con ella (U6e); sin ella, mira los eventos en cada tic de un
// temporizador de 10 ms. Toda su memoria es un objeto DMA del permiso, sin caché; su dirección de bus es la física más
// el desplazamiento del permiso (0 en QEMU; en la Pi 5, 0x10_0000_0000, el `dma-ranges` del PCIe del RP1).
//
// Para una placa nueva (la Pi 5) dice en el registro lo que ve, paso a paso: la versión y los parámetros, si el DMA
// llega (una orden vacía, 4.6.2), los puertos (PORTSC), cada aparato con su fabricante y producto, y cada tecla.

use dashos::println;
use dashos::protocolo::usb_prestado as up;

dashos::programa!(principal);

/// Una línea del registro con el nombre de esta copia del controlador ("xhci", "xhci0", "xhci1").
macro_rules! decir {
    ($($t:tt)*) => { println!("  [{}] {}", NOMBRE, format_args!($($t)*)) };
}

const VA: usize = 0x0090_0000;

// Capacidad (5.3).
const CAPLENGTH: usize = 0x00;
const HCIVERSION: usize = 0x02;
const HCSPARAMS1: usize = 0x04;
const HCSPARAMS2: usize = 0x08;
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
/// PP (5.4.8): el puerto tiene corriente; con PPC (HCCPARAMS1 bit 3) lo pone el programa.
const PORT_PP: u32 = 1 << 9;
const STS_HSE: u32 = 1 << 2;
const PORT_PRC: u32 = 1 << 21;
/// Bits de PORTSC que se borran escribiendo 1 (y PED, que apaga el puerto): fuera al escribir otra cosa.
const PORT_RW1C: u32 = 0x00FE_0002;
const HCCPARAMS1: usize = 0x10;
const DBOFF: usize = 0x14;
const RTSOFF: usize = 0x18;
const CRCR: usize = 0x18;
const DCBAAP: usize = 0x30;
const CONFIG: usize = 0x38;

// Memoria (un objeto DMA de 256 KB): dónde va cada cosa.
const VA_DMA: usize = 0x30_0000_0000;
const TAM_DMA: usize = 256 * 1024;
const M_DCBAA: usize = 0x0000;
const M_ERST: usize = 0x0400;
/// La tabla de los búferes de trabajo del controlador (scratchpad, 4.20; la entrada 0 de la DCBAA apunta a ella) y los
/// búferes, una página cada uno, desde M_SCRATCH. QEMU no pide ninguno; un DWC3 puede pedirlos.
const M_SCRATCH_TABLA: usize = 0x0C00;
const M_SCRATCH: usize = 0x2_0000;
const MAX_SCRATCH: usize = 32;
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
const T_ORDEN_VACIA: u32 = 23;
const T_NOOP: u32 = 8;
const T_DESACTIVAR_RANURA: u32 = 10;
const T_REINICIAR_PUNTO: u32 = 14;
const T_PARAR_PUNTO: u32 = 15;
const T_PONER_DEQUEUE: u32 = 16;
const T_EV_TRANSFERENCIA: u32 = 32;
const T_EV_ORDEN: u32 = 33;
const T_EV_PUERTO: u32 = 34;
/// Bits de un TRB de transferencia (6.4.1): interrumpir en un paquete corto (ISP) y al acabar (IOC).
const TRB_ISP: u32 = 1 << 2;
const TRB_IOC: u32 = 1 << 5;
/// Los bits de cambio de PORTSC (CSC a CEC): se borran escribiendo 1.
const PORT_CAMBIOS: u32 = 0x00FE_0000;

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
    va: usize,
    bus: u64,
    i: usize,
    ciclo: u32,
}

/// Escribe un TRB en `va` (memoria DMA proyectada aquí, sin caché).
fn poner_trb(va: usize, parametro: u64, estado: u32, control: u32) {
    // SAFETY: `va` es un TRB entero dentro de un objeto DMA proyectado por este programa (lo garantiza `Anillo`).
    unsafe {
        core::ptr::write_volatile(va as *mut u64, parametro.to_le());
        core::ptr::write_volatile((va + 8) as *mut u32, estado.to_le());
        dashos::barrera::escrituras();
        core::ptr::write_volatile((va + 12) as *mut u32, control.to_le()); // el ciclo, lo último
    }
}

fn leer_trb_control(va: usize) -> u32 {
    // SAFETY: como `poner_trb`.
    u32::from_le(unsafe { core::ptr::read_volatile((va + 12) as *const u32) })
}

impl Anillo {
    /// Un anillo en la memoria de siempre (`VA_DMA` + `off`).
    fn nuevo(off: usize, bus_base: u64) -> Self {
        Self::en(VA_DMA + off, bus_base + off as u64)
    }

    /// Un anillo en `va` (dirección de bus `bus`), borrado entero antes: lo que quedara de un uso anterior (otro aparato
    /// en la misma ranura, un punto reconfigurado) no debe parecerle al controlador un TRB con el ciclo bueno.
    fn en(va: usize, bus: u64) -> Self {
        for i in (0..16 * TRBS).step_by(8) {
            // SAFETY: el anillo entero (16 · TRBS bytes) está dentro de un objeto DMA proyectado aquí.
            unsafe { core::ptr::write_volatile((va + i) as *mut u64, 0) };
        }
        // El enlace del final: al principio, con "dar la vuelta al ciclo" (bit 1).
        poner_trb(va + 16 * (TRBS - 1), bus, 0, (T_ENLACE << 10) | 2);
        Self { va, bus, i: 0, ciclo: 1 }
    }

    /// Pone un TRB y devuelve su dirección de bus.
    fn poner(&mut self, parametro: u64, estado: u32, control: u32) -> u64 {
        let donde = self.bus + 16 * self.i as u64;
        poner_trb(self.va + 16 * self.i, parametro, estado, control | self.ciclo);
        self.i += 1;
        if self.i == TRBS - 1 {
            // El enlace pasa a ser del ciclo actual, y el ciclo da la vuelta.
            poner_trb(self.va + 16 * (TRBS - 1), self.bus, 0, (T_ENLACE << 10) | 2 | self.ciclo);
            self.ciclo ^= 1;
            self.i = 0;
        }
        donde
    }

    /// Dónde irá el siguiente TRB y con qué ciclo (para Set TR Dequeue Pointer, 4.6.10).
    fn siguiente(&self) -> (u64, u32) {
        (self.bus + 16 * self.i as u64, self.ciclo)
    }

    /// La dirección aquí del TRB de bus `b`, si es de este anillo.
    fn va_de(&self, b: u64) -> Option<usize> {
        (b >= self.bus && b < self.bus + 16 * TRBS as u64 && (b - self.bus) % 16 == 0).then(|| self.va + (b - self.bus) as usize)
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
    /// Para el registro, si un aparato no se configura: en qué paso se quedó y el último código de terminación.
    paso: &'static str,
    codigo: u32,
    /// Los eventos que llegaron mientras se esperaba otro (una orden, una transferencia de control): se atienden
    /// después, en el bucle. Si se llena, se pierden (y se cuentan).
    guardados: [Evento; GUARDADOS],
    g_ini: usize,
    g_n: usize,
    perdidos: u64,
}

const GUARDADOS: usize = 64;

fn tipo_de(ev: &Evento) -> u32 {
    (ev.2 >> 10) & 0x3f
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
        dashos::barrera::completa();
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

    fn guardar(&mut self, ev: Evento) {
        if self.g_n == GUARDADOS {
            self.perdidos += 1;
            return;
        }
        self.guardados[(self.g_ini + self.g_n) % GUARDADOS] = ev;
        self.g_n += 1;
    }

    /// El siguiente evento por atender: primero los guardados, en orden.
    fn siguiente(&mut self) -> Option<Evento> {
        if self.g_n > 0 {
            let ev = self.guardados[self.g_ini];
            self.g_ini = (self.g_ini + 1) % GUARDADOS;
            self.g_n -= 1;
            return Some(ev);
        }
        self.evento()
    }

    /// Espera un evento que cumpla `vale`, como mucho `ms`; los demás se guardan para el bucle (transferencias de
    /// otros aparatos, cambios de puerto).
    fn esperar_que(&mut self, ms: u64, vale: impl Fn(&Evento) -> bool) -> Option<Evento> {
        let fin = dashos::ahora_us() + ms * 1000;
        while dashos::ahora_us() < fin {
            if let Some(ev) = self.evento() {
                if vale(&ev) {
                    return Some(ev);
                }
                self.guardar(ev);
            }
        }
        None
    }

    fn esperar_evento(&mut self, tipo: u32, ms: u64) -> Option<Evento> {
        self.esperar_que(ms, |ev| tipo_de(ev) == tipo)
    }

    /// Una orden: devuelve (código de terminación, ranura).
    fn orden(&mut self, parametro: u64, control: u32) -> Option<(u32, u32)> {
        self.ordenes.poner(parametro, 0, control);
        self.timbre(0, 0);
        let ev = self.esperar_evento(T_EV_ORDEN, 500)?;
        self.codigo = ev.1 >> 24;
        Some((ev.1 >> 24, ev.2 >> 24))
    }
}

/// Lo de cada aparato con dirección.
#[derive(Clone, Copy)]
struct Ranura {
    id: u32,
    /// El número con que se ofrece para prestarlo (no se repite: al volver, otro).
    aparato: u32,
    /// El puerto raíz, el traductor y, detrás de un concentrador, cuál (su sitio en `lista`) y en qué puerto suyo.
    puerto: u32,
    tt: Option<(u32, u32)>,
    padre: Option<(usize, u32)>,
    configuraciones: u8,
    /// Es un concentrador (clase 9): sus puertos se recorren aparte (U6k).
    hub: bool,
    velocidad: u32,
    ruta: u32,
    ep0: Anillo,
    int: Anillo,
    dci: u32,
    mps: u32,
    teclado: bool,
    /// Una tableta (puntero absoluto, como la `usb-tablet` de QEMU): dónde va cada cosa en su informe
    /// (`dashos::hid_informe`). Sin ella, con `teclado` falso, es un ratón de arranque.
    tableta: Option<dashos::hid_informe::Tableta>,
    activo: bool,
    /// Del descriptor de aparato: fabricante, producto y clase; y la primera interfaz (clase, subclase, protocolo).
    vid: u16,
    pid: u16,
    clase: u8,
    interfaz: (u8, u8, u8),
}

/// Un aparato para el registro: "aparato 046d:c31c, clase 0, interfaz 3/1/1".
struct Aparato<'a>(&'a Ranura);

impl core::fmt::Display for Aparato<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let r = self.0;
        write!(f, "aparato {:04x}:{:04x}, clase {}, interfaz {}/{}/{}", r.vid, r.pid, r.clase, r.interfaz.0, r.interfaz.1, r.interfaz.2)
    }
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
        let datos = self.bus + (base_ranura(k) + R_DATOS) as u64;
        self.control_en(r, setup, largo, datos)
    }

    /// Como `control`, con los datos de entrada en `datos` (dirección de bus). Espera su evento (el de su ranura y su
    /// punto 0); lo demás que llegue se guarda.
    fn control_en(&mut self, r: &mut Ranura, setup: [u8; 8], largo: u32, datos: u64) -> Option<u32> {
        let tipo_transferencia = if largo > 0 { 3 } else { 0 }; // 3: datos de entrada
        r.ep0.poner(u64::from_le_bytes(setup), 8, (T_SETUP << 10) | (1 << 6) | (tipo_transferencia << 16));
        if largo > 0 {
            r.ep0.poner(datos, largo, (T_DATOS << 10) | (1 << 16));
        }
        // Estado: al revés que los datos (de salida tras entrada; de entrada si no hubo datos), con aviso al acabar.
        r.ep0.poner(0, 0, (T_ESTADO << 10) | (1 << 5) | if largo > 0 { 0 } else { 1 << 16 });
        self.timbre(r.id as usize, 1);
        let id = r.id;
        let ev = self.esperar_que(500, |ev| tipo_de(ev) == T_EV_TRANSFERENCIA && ev.2 >> 24 == id && (ev.2 >> 16) & 0x1f == 1)?;
        let codigo = ev.1 >> 24;
        self.codigo = codigo;
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
    // Con la tabla de nombres (lo que da `inicio`), por su nombre: la interrupción se llama como el controlador.
    let tabla = dashos::llaves::Tabla::leer();
    let (irq, canal) = if tabla.cuantas() > 0 {
        (tabla.nombre(3).is_some_and(|n| n == NOMBRE.as_bytes()).then_some(3), tabla.buscar("ventanas"))
    } else if existe(4) {
        (Some(3), Some(4))
    } else {
        (None, existe(3).then_some(3))
    };
    let usb_dominio = tabla.buscar("usb-dominio");
    if dashos::registros_proyectar(2, VA) != 0 {
        decir!("sin registros");
        return 1;
    }
    let cap = leer(CAPLENGTH);
    let op = (cap & 0xff) as usize;
    let version = (leer(HCIVERSION & !3) >> 16) & 0xffff;
    let params = leer(HCSPARAMS1);
    let puertos = (params >> 24) as usize;
    let ranuras = ((params & 0xff) as usize).min(MAX_RANURAS);
    let hcc1 = leer(HCCPARAMS1);
    let ctx = if hcc1 & (1 << 2) != 0 { 64 } else { 32 };
    let (ac64, ppc) = (hcc1 & 1 != 0, hcc1 & (1 << 3) != 0);
    // Los búferes de trabajo que pide (5.3.4: los 5 bits altos en 25:21 y los 5 bajos en 31:27).
    let hcs2 = leer(HCSPARAMS2);
    let scratch = ((((hcs2 >> 21) & 0x1f) << 5) | (hcs2 >> 27)) as usize;
    decir!(
        "xHCI {:x}.{:02x}: {} puertos, {} ranuras, contexto de {} bytes, {} bits, scratch {}, PPC {}, USBSTS {:#x}",
        version >> 8, version & 0xff, puertos, params & 0xff, ctx, if ac64 { 64 } else { 32 }, scratch, if ppc { "sí" } else { "no" }, leer(op + USBSTS)
    );
    if scratch > MAX_SCRATCH {
        decir!("MAL: pide {} búferes de trabajo y solo hay sitio para {}", scratch, MAX_SCRATCH);
        return 7;
    }
    // Parar (por si el firmware lo dejó en marcha) y reiniciar (4.2).
    escribir(op + USBCMD, leer(op + USBCMD) & !CMD_RUN);
    if !esperar(20, || leer(op + USBSTS) & STS_HCH != 0) {
        decir!("MAL: no se para (USBSTS {:#x})", leer(op + USBSTS));
        return 2;
    }
    escribir(op + USBCMD, CMD_HCRST);
    if !esperar(1000, || leer(op + USBCMD) & CMD_HCRST == 0 && leer(op + USBSTS) & STS_CNR == 0) {
        decir!("MAL: no acaba de reiniciarse (USBCMD {:#x}, USBSTS {:#x})", leer(op + USBCMD), leer(op + USBSTS));
        return 3;
    }
    // Con el controlador reiniciado, lo que dejara en cuarentena uno anterior puede volver (M5).
    dashos::dma_liberar(1);
    let (m, bus) = dashos::memoria_dma(1, TAM_DMA);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_DMA, true) != 0 {
        decir!("MAL: sin memoria DMA");
        return 4;
    }
    // Un controlador de 32 bits no alcanza una dirección de bus por encima de 4 GB (en la Pi 5, todas: física + 64 GB).
    if !ac64 && bus >> 32 != 0 {
        decir!("MAL: el controlador es de 32 bits y su memoria DMA está en el bus {:#x}", bus);
        return 8;
    }
    let mut x = Xhci {
        db: leer(DBOFF) as usize & !3,
        rt: leer(RTSOFF) as usize & !0x1f,
        bus,
        ctx,
        ordenes: Anillo::nuevo(M_ORDENES, bus),
        ev_i: 0,
        ev_ciclo: 1,
        paso: "",
        codigo: 0,
        guardados: [(0, 0, 0); GUARDADOS],
        g_ini: 0,
        g_n: 0,
        perdidos: 0,
    };
    // Los búferes de trabajo: su tabla, en la entrada 0 de la DCBAA (6.6).
    if scratch > 0 {
        for i in 0..scratch {
            x.poner64(M_SCRATCH_TABLA + 8 * i, bus + (M_SCRATCH + 4096 * i) as u64);
        }
        x.poner64(M_DCBAA, bus + M_SCRATCH_TABLA as u64);
    }
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
        decir!("MAL: no arranca (USBSTS {:#x})", leer(op + USBSTS));
        return 5;
    }
    // ¿Llega el DMA? Una orden vacía (6.4.3.1) hace que el controlador lea el anillo de órdenes y escriba un evento: si
    // no contesta, el controlador no alcanza su memoria (la ventana de entrada del PCIe, el bus maestro...).
    match x.orden(0, T_ORDEN_VACIA << 10) {
        Some((1, _)) => decir!("DMA bien: contesta a una orden vacía (memoria en el bus {:#x})", bus),
        r => {
            let sts = leer(op + USBSTS);
            decir!(
                "MAL: la orden vacía {} (USBSTS {:#x}{}; memoria en el bus {:#x})",
                if r.is_some() { "acabó con error" } else { "no contesta" },
                sts,
                if sts & STS_HSE != 0 { ", HSE: el DMA no llega a la memoria" } else { "" },
                bus
            );
            return 9;
        }
    }
    // Corriente a los puertos, si la pone el programa (PPC), y un rato para que los aparatos se conecten: tras el
    // reinicio, uno de USB 2 tarda en dar CCS y uno de USB 3 tiene que entrenar el enlace (4.19).
    if ppc {
        for p in 1..=puertos {
            let reg = op + PORTSC + 0x10 * (p - 1);
            if leer(reg) & PORT_PP == 0 {
                escribir(reg, (leer(reg) & !PORT_RW1C) | PORT_PP);
            }
        }
        dashos::dormir_us(20_000);
    }
    let algo = || (1..=puertos).any(|p| leer(op + PORTSC + 0x10 * (p - 1)) & PORT_CCS != 0);
    if esperar(1500, algo) {
        dashos::dormir_us(200_000);
    }
    for p in 1..=puertos {
        let sc = leer(op + PORTSC + 0x10 * (p - 1));
        decir!(
            "puerto {}: PORTSC {:#x} ({}{}, velocidad {}, estado del enlace {})",
            p, sc,
            if sc & PORT_CCS != 0 { "algo conectado" } else { "nada" },
            if sc & PORT_PP != 0 { "" } else { ", sin corriente" },
            (sc >> 10) & 0xf, (sc >> 5) & 0xf
        );
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
            decir!("puerto {}: no se activa (PORTSC {:#x})", p, leer(reg));
            continue;
        }
        escribir(reg, (leer(reg) & !PORT_RW1C) | PORT_PRC);
        let velocidad_p = (leer(reg) >> 10) & 0xf;
        match configurar(&mut x, p as u32, velocidad_p, n, 0, None) {
            Some(r) => {
                lista[n] = Some(r);
                n += 1;
                if r.hub {
                    decir!("puerto {}: ranura {}, concentrador, {}", p, r.id, Aparato(&r));
                    concentrador(&mut x, p as u32, n - 1, None, &mut lista, &mut n, ranuras);
                } else {
                    decir!("puerto {}: ranura {}, {}, {}", p, r.id, que_es(&r), Aparato(&r));
                }
            }
            None => decir!("puerto {}: MAL, no se pudo configurar (en {}: código {}; USBSTS {:#x})", p, x.paso, x.codigo, leer(op + USBSTS)),
        }
    }
    let teclados = lista.iter().flatten().filter(|r| r.activo && r.teclado).count();
    let ratones = lista.iter().flatten().filter(|r| r.activo && !r.teclado && r.tableta.is_none()).count();
    let tabletas = lista.iter().flatten().filter(|r| r.activo && r.tableta.is_some()).count();
    decir!("versión {:x}.{:02x}, {} puertos; HID de arranque: {} teclado y {} ratón; {} tableta", version >> 8, version & 0xff, puertos, teclados, ratones, tabletas);
    // Los eventos se miran al llegar la interrupción (o cada 10 ms sin ella): los informes que llegan van al
    // compositor y se pide el siguiente.
    let mut entrada = dashos::hid::Entrada::nueva(canal, NOMBRE);
    let (a, b) = dashos::canal_crear();
    let irq = irq.filter(|&i| dashos::interrupcion_asociar(i, b) == 0);
    if irq.is_none() && dashos::temporizador_crear(b, 10_000, 10_000) <= 0 {
        return 6;
    }
    decir!("avisos {}", if irq.is_some() { "por su interrupción" } else { "cada 10 ms" });
    if irq.is_some() {
        escribir(ir, IMAN_IE | IMAN_IP);
        escribir(op + USBCMD, CMD_RUN | CMD_INTE);
    }
    let mut pm = Prestamos {
        dominio: None,
        para_dominio: b,
        prestados: [None, None],
        salida: [[0; up::MAX_MENSAJE]; SALIDA],
        s_ini: 0,
        s_n: 0,
        perdido: false,
        rechazos: 0,
    };
    // Los dominios piden por "usb-dominio": un hilo lo lee y pasa cada HOLA (con su llave) al canal propio.
    if let Some(ud) = usb_dominio {
        let reenvia = dashos::llave_duplicar(b, dashos::ESCRIBIR | dashos::TRANSFERIR, M_HOLA);
        // SAFETY: la pila es solo de ese hilo, que no acaba; nadie más la toca.
        let pila = unsafe { &mut *core::ptr::addr_of_mut!(PILA_HOLA) };
        if reenvia <= 0 || dashos::hilos::crear(hilo_hola, ud as u64 | (reenvia as u64) << 32, pila, 0) < 0 {
            decir!("MAL: no se puede atender a los dominios");
        } else {
            decir!("presta aparatos a un dominio de controladores, si lo pide");
        }
    }
    let mut buf = [0u8; 64];
    let mut ll = [0u32; 2];
    let mut recibido: (i64, usize, u64) = (-1, 0, 0);
    let mut n_aviso = 0; // el tamaño del último aviso: 4, de la interrupción; 8, del temporizador
    // Con la interrupción, un temporizador de una vez mientras haya eventos sin entregar al compositor (E4).
    let mut reintento: Option<u32> = None;
    let mut hse_dicho = false;
    loop {
        if !hse_dicho && leer(op + USBSTS) & STS_HSE != 0 {
            decir!("MAL: error del sistema anfitrión (HSE, USBSTS {:#x}): el DMA dejó de llegar a la memoria", leer(op + USBSTS));
            hse_dicho = true;
        }
        if n_aviso == 8 {
            if let Some(t) = reintento.take() {
                dashos::cerrar(t);
            }
        }
        entrada.reintentar();
        pm.reintentar();
        // Lo que llegó de un dominio.
        let (n, k, marca) = recibido;
        if n >= 0 && marca == M_HOLA && k == 1 {
            nuevo_dominio(&mut x, op, &mut pm, &mut lista, ll[0]);
        } else if n >= 0 && marca == M_DOMINIO && pm.dominio.is_some() {
            atender_dominio(&mut x, op, &mut pm, &mut lista, &buf[..(n as usize).min(buf.len())]);
            for &l in &ll[..k.min(ll.len())] {
                dashos::cerrar(l);
            }
        } else if n >= 0 && marca != 0 {
            for &l in &ll[..k.min(ll.len())] {
                dashos::cerrar(l);
            }
        }
        if pm.perdido {
            pm.perdido = false;
            if let Some(d) = pm.dominio.take() {
                dashos::cerrar(d);
            }
            for i in 0..MAX_PRESTADOS {
                acabar_prestamo(&mut x, op, &mut pm, &mut lista, i, true, "el dominio se fue");
            }
            decir!("el dominio se fue: lo prestado vuelve a DashOS");
        }
        // Borrar el aviso antes de mirar los eventos: uno que llegue mientras vuelve a ponerlo (4.17.5).
        if irq.is_some() {
            escribir(op + USBSTS, STS_EINT);
            escribir(ir, IMAN_IE | IMAN_IP);
        }
        while let Some(ev) = x.siguiente() {
            if tipo_de(&ev) == T_EV_PUERTO {
                puerto_cambia(&mut x, op, &mut pm, &mut lista, ((ev.0 >> 24) & 0xff) as u32, ranuras);
                continue;
            }
            if tipo_de(&ev) != T_EV_TRANSFERENCIA {
                continue;
            }
            let (ranura, dci, codigo) = (ev.2 >> 24, (ev.2 >> 16) & 0x1f, ev.1 >> 24);
            if let Some(i) = pm.prestados.iter().position(|p| p.as_ref().is_some_and(|p| lista[p.k].is_some_and(|r| r.id == ranura))) {
                evento_prestado(&mut x, &mut pm, i, &mut lista, ev);
                continue;
            }
            let Some(k) = lista.iter().position(|r| r.is_some_and(|r| r.id == ranura && r.dci == dci)) else { continue };
            let r = lista[k].as_mut().unwrap();
            if codigo == 1 || codigo == 13 {
                let largo = r.mps.saturating_sub(ev.1 & 0xff_ffff) as usize;
                let mut informe = [0u8; 16];
                for (i, b) in informe.iter_mut().enumerate().take(largo.min(16)) {
                    *b = unsafe { core::ptr::read_volatile((VA_DMA + base_ranura(k) + R_INFORME + i) as *const u8) };
                }
                let informe = &informe[..largo.min(16)];
                if r.teclado {
                    entrada.teclado(r.aparato, informe)
                } else if let Some(t) = r.tableta {
                    if let Some(l) = dashos::hid_informe::leer(&t, informe) {
                        entrada.tableta(l)
                    }
                } else {
                    entrada.raton(informe)
                }
            }
            // El siguiente informe.
            r.int.poner(bus + (base_ranura(k) + R_INFORME) as u64, r.mps, (T_NORMAL << 10) | (1 << 5) | (1 << 2));
            x.timbre(r.id as usize, r.dci);
        }
        // La interrupción se vuelve a permitir con el aviso ya borrado; si llegó otro evento, salta otra vez.
        if let Some(i) = irq.filter(|_| n_aviso == 4) {
            dashos::interrupcion_terminar(i);
        }
        // tecla-pegada: un teclado desenchufado o prestado a un dominio suelta lo que tenía pulsado (su soltar ya no
        // llegará por aquí).
        entrada.revisar_teclados(|a| {
            lista.iter().enumerate().any(|(k, r)| {
                r.is_some_and(|r| r.aparato == a && r.teclado) && !pm.prestados.iter().flatten().any(|p| p.k == k)
            })
        });
        if irq.is_some() && (entrada.pendiente() || pm.s_n > 0) && reintento.is_none() {
            let t = dashos::temporizador_crear(b, 10_000, 0);
            reintento = (t > 0).then_some(t as u32);
        }
        recibido = dashos::canal_leer_marca(a, &mut buf, &mut ll);
        n_aviso = if recibido.2 == 0 { recibido.0 } else { 0 };
    }
}

static mut PILA_HOLA: [u8; 8192] = [0; 8192];

/// El hilo que lee "usb-dominio": cada `HOLA` (con su llave) va al canal propio, con su marca; lo demás (lo que diga
/// `inicio` de sus clientes) se tira.
fn hilo_hola(arg: u64) -> u64 {
    let (lee, reenvia) = (arg as u32, (arg >> 32) as u32);
    loop {
        let mut b = [0u8; 64];
        let mut ll = [0u32; 4];
        let (n, k) = dashos::canal_leer(lee, &mut b, &mut ll);
        if n < 0 {
            return 1;
        }
        let k = k.min(ll.len());
        let es_hola = k == 1 && up::Mensaje::leer(&b[..(n as usize).min(b.len())]) == Some(up::Mensaje::Hola);
        if es_hola && dashos::canal_escribir(reenvia, &b[..(n as usize).min(b.len())], &ll[..1]) == 0 {
            continue;
        }
        for &l in &ll[..k] {
            dashos::cerrar(l);
        }
    }
}

/// Un dominio nuevo (o el mismo, relanzado): lo que tuviera prestado el anterior vuelve, se le contesta y se le ofrece
/// lo que hay.
fn nuevo_dominio(x: &mut Xhci, op: usize, pm: &mut Prestamos, lista: &mut [Option<Ranura>; MAX_RANURAS], llave: u32) {
    if let Some(d) = pm.dominio.take() {
        dashos::cerrar(d);
    }
    pm.s_n = 0;
    for i in 0..MAX_PRESTADOS {
        acabar_prestamo(x, op, pm, lista, i, true, "llega otro dominio");
    }
    let para = dashos::llave_duplicar(pm.para_dominio, dashos::ESCRIBIR | dashos::TRANSFERIR, M_DOMINIO);
    let mut b = [0u8; up::MAX_MENSAJE];
    let n = up::Mensaje::Hola.escribir(&mut b);
    if para <= 0 || dashos::canal_escribir(llave, &b[..n], &[para as u32]) != 0 {
        decir!("un dominio saluda, pero no se le puede contestar");
        if para > 0 {
            dashos::cerrar(para as u32);
        }
        dashos::cerrar(llave);
        return;
    }
    pm.dominio = Some(llave);
    decir!("un dominio de controladores pide aparatos prestados");
    for r in lista.iter().flatten() {
        ofrecer(pm, r);
    }
}

fn que_es(r: &Ranura) -> &'static str {
    if r.dci == 0 {
        "sin teclado ni ratón de arranque"
    } else if r.teclado {
        "teclado"
    } else if r.tableta.is_some() {
        "tableta"
    } else {
        "ratón"
    }
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
            decir!("concentrador {}, puerto {}: no se activa", h.id, i);
            continue;
        };
        // Velocidades de xHCI (7.2.2.1.1): plena 1, baja 2, alta 3.
        let velocidad = if e1 & (1 << 9) != 0 { 2 } else if e1 & (1 << 10) != 0 { 3 } else { 1 };
        // Un aparato lento detrás de un concentrador de alta velocidad va por su traductor.
        let tt_hijo = if h.velocidad == 3 && velocidad < 3 { Some((h.id, i)) } else { tt };
        let ruta = h.ruta | i << (4 * salto);
        match configurar(x, p, velocidad, *n, ruta, tt_hijo) {
            Some(mut r) => {
                r.padre = Some((k, i));
                lista[*n] = Some(r);
                *n += 1;
                if r.hub && salto < 4 {
                    decir!("puerto {} ruta {:x}: ranura {}, concentrador, {}", p, ruta, r.id, Aparato(&r));
                    concentrador(x, p, *n - 1, tt_hijo, lista, n, max);
                } else {
                    decir!("puerto {} ruta {:x}: ranura {}, {}, {}", p, ruta, r.id, que_es(&r), Aparato(&r));
                }
            }
            None => decir!("puerto {} ruta {:x}: MAL, no se pudo configurar (en {}: código {})", p, ruta, x.paso, x.codigo),
        }
    }
    lista[k] = Some(h);
}

/// Pide una ranura, da dirección al aparato del puerto `p` y, si es un teclado o ratón de arranque, configura su punto
/// de interrupción y pide el primer informe (4.3.2 a 4.3.5). `k` es su sitio en la memoria.
/// `ruta` es la cadena de ruta (4 bits por salto de concentrador, 4.3.3; 0 en un puerto raíz) y `tt`, el traductor
/// de un aparato de baja o plena velocidad detrás de un concentrador de alta (ranura y puerto del concentrador).
fn configurar(x: &mut Xhci, p: u32, velocidad: u32, k: usize, ruta: u32, tt: Option<(u32, u32)>) -> Option<Ranura> {
    x.paso = "pedir ranura";
    x.codigo = 0;
    let (codigo, id) = x.orden(0, T_PEDIR_RANURA << 10)?;
    // El número lo da el controlador: fuera de las ranuras que se le dieron, escribiría fuera de la tabla (S7).
    if codigo != 1 || id == 0 || id as usize > MAX_RANURAS {
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
    let tt_par = tt;
    let tt = tt.map_or(0, |(ranura, puerto)| ranura | puerto << 8);
    x.poner32(e + c, (velocidad << 20) | (1 << 27) | ruta);
    x.poner32(e + c + 4, p << 16);
    x.poner32(e + c + 8, tt);
    x.poner32(e + 2 * c + 4, (3 << 1) | (4 << 3) | (mps0 << 16));
    x.poner64(e + 2 * c + 8, ep0.bus | 1);
    x.poner32(e + 2 * c + 16, 8);
    x.paso = "dar dirección";
    let (codigo, _) = x.orden(bus + e as u64, (T_DIRECCION << 10) | (id << 24))?;
    if codigo != 1 {
        return None;
    }
    let mut r = Ranura {
        id,
        aparato: 0,
        puerto: p,
        tt: tt_par,
        padre: None,
        configuraciones: 0,
        hub: false,
        velocidad,
        ruta,
        ep0,
        int,
        dci: 0,
        mps: 0,
        teclado: false,
        tableta: None,
        activo: false,
        vid: 0,
        pid: 0,
        clase: 0,
        interfaz: (0, 0, 0),
    };
    // SAFETY: solo lo toca el hilo principal (el del préstamo no lo mira).
    r.aparato = SIGUIENTE_APARATO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    // Descriptor de aparato (18) y de configuración (9 y después entero).
    x.paso = "descriptor de aparato";
    x.control(&mut r, k, [0x80, 6, 0, 1, 0, 0, 18, 0], 18)?;
    let da = x.datos(k, 18);
    r.hub = da[4] == 9;
    r.configuraciones = da[17];
    (r.vid, r.pid, r.clase) = (u16::from_le_bytes([da[8], da[9]]), u16::from_le_bytes([da[10], da[11]]), da[4]);
    x.paso = "descriptor de configuración";
    let n = x.control(&mut r, k, [0x80, 6, 0, 2, 0, 0, 9, 0], 9)?;
    let d = x.datos(k, n as usize);
    let total = u16::from_le_bytes([d[2], d[3]]).min(256) as u32;
    let valor = d[5];
    let n = x.control(&mut r, k, [0x80, 6, 0, 2, 0, 0, total as u8, (total >> 8) as u8], total)? as usize;
    let d = x.datos(k, n);
    // Interfaz HID de arranque (clase 3, subclase 1, protocolo 1 teclado o 2 ratón) y su punto de entrada de
    // interrupción. O, si no hay de arranque, una HID cualquiera (protocolo 0) que puede ser una tableta: se mira su
    // descriptor de informe (el largo, en su descriptor HID, 0x21).
    let (mut i, mut interfaz, mut hallado) = (0, None, None);
    let (mut otra, mut otra_hallada, mut largo_informe) = (None::<u8>, None, 0u16);
    while i + 1 < n && d[i] != 0 {
        match d[i + 1] {
            4 if i + 8 < n => {
                if r.interfaz == (0, 0, 0) {
                    r.interfaz = (d[i + 5], d[i + 6], d[i + 7]);
                }
                interfaz = (d[i + 5] == 3 && d[i + 6] == 1 && (d[i + 7] == 1 || d[i + 7] == 2)).then_some((d[i + 2], d[i + 7] == 1));
                otra = (d[i + 5] == 3 && d[i + 7] == 0).then_some(d[i + 2]);
            }
            0x21 if i + 8 < n && otra.is_some() && otra_hallada.is_none() && d[i + 6] == 0x22 => {
                largo_informe = u16::from_le_bytes([d[i + 7], d[i + 8]]);
            }
            5 if i + 6 < n => {
                if let Some((ifz, teclado)) = interfaz {
                    if d[i + 2] & 0x80 != 0 && d[i + 3] & 3 == 3 && hallado.is_none() {
                        let mps = u16::from_le_bytes([d[i + 4], d[i + 5]]) as u32 & 0x7ff;
                        hallado = Some((ifz, teclado, (d[i + 2] & 0xf) as u32, mps, d[i + 6]));
                    }
                } else if let Some(ifz) = otra
                    && d[i + 2] & 0x80 != 0
                    && d[i + 3] & 3 == 3
                    && otra_hallada.is_none()
                {
                    let mps = u16::from_le_bytes([d[i + 4], d[i + 5]]) as u32 & 0x7ff;
                    otra_hallada = Some((ifz, false, (d[i + 2] & 0xf) as u32, mps, d[i + 6]));
                }
            }
            _ => {}
        }
        i += (d[i] as usize).max(2);
    }
    x.paso = "SET_CONFIGURATION";
    x.control(&mut r, k, [0x00, 9, valor, 0, 0, 0, 0, 0], 0)?; // SET_CONFIGURATION
    if r.hub {
        return Some(r);
    }
    // Sin teclado ni ratón de arranque: una tableta, si su descriptor de informe lo dice (hasta 256 bytes, lo que cabe
    // en los datos de la ranura; uno más largo no se mira).
    if hallado.is_none()
        && let Some(h) = otra_hallada
        && (1..=256).contains(&largo_informe)
    {
        x.paso = "descriptor de informe";
        let n = x.control(&mut r, k, [0x81, 6, 0, 0x22, h.0, 0, largo_informe as u8, (largo_informe >> 8) as u8], largo_informe as u32)? as usize;
        let d = x.datos(k, n);
        r.tableta = dashos::hid_informe::tableta(&d[..n.min(256)]);
        if r.tableta.is_some() && h.3 <= dashos::hid_informe::MAX_INFORME {
            hallado = Some(h);
        } else {
            r.tableta = None;
        }
    }
    let Some((ifz, teclado, num, mps, b_intervalo)) = hallado else { return Some(r) };
    if r.tableta.is_none() {
        x.paso = "SET_PROTOCOL";
        x.control(&mut r, k, [0x21, 0x0B, 0, 0, ifz, 0, 0, 0], 0)?; // SET_PROTOCOL: arranque
    }
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


static SIGUIENTE_APARATO: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);

// ---- Prestar aparatos a un dominio (S35, paso 2; `protocolo::usb_prestado`) ----

/// Marcas de lo que llega por el canal propio: el `HOLA` de un dominio (lo reenvía el hilo que lee "usb-dominio") y lo
/// que pide después.
const M_HOLA: u64 = 0x5542_0001;
const M_DOMINIO: u64 = 0x5542_0002;
const MAX_PRESTADOS: usize = 2;
/// La memoria propia de cada préstamo (DMA, no se presta): un anillo de 1 KB por punto (en su DCI) y, desde
/// `P_CONFIG`, el descriptor de configuración en uso (hasta `MAX_CONFIG`).
const VA_PRESTADO: usize = 0x31_0000_0000;
const VA_BUFERES: usize = 0x32_0000_0000;
const ENTRE_PRESTADOS: usize = 0x100_0000;
const TAM_PRIVADA: usize = 64 * 1024;
const P_CONFIG: usize = 32 * 1024;
const MAX_CONFIG: usize = 4096;
/// Mensajes al dominio que esperan sitio en su cola.
const SALIDA: usize = 64;

/// Una transferencia en marcha de un aparato prestado: sus TRB (de bus), el que avisa al acabar (`ultimo`), el de
/// datos (para el largo de un paquete corto) y lo hecho, si ya se sabe.
#[derive(Clone, Copy)]
struct Td {
    etiqueta: u32,
    dci: u32,
    largo: u32,
    trbs: [u64; 4],
    n: usize,
    ultimo: u64,
    datos: u64,
    ciclo0: u32,
    hecho: Option<u32>,
    orden: u64,
}

struct Prestado {
    aparato: u32,
    /// Su sitio en `Prestamos::prestados` (y en las direcciones de su memoria).
    indice: usize,
    /// Su sitio en `lista`.
    k: usize,
    privada: u32,
    privada_bus: u64,
    buferes: u32,
    buferes_bus: u64,
    /// Por DCI (2 a 31): su anillo, su tipo de punto (6.2.3: 2 masivo de salida, 3 interrupción de salida, 6 masivo de
    /// entrada, 7 interrupción de entrada; 0 sin configurar) y su paquete máximo.
    anillos: [Option<Anillo>; 32],
    tipos: [u8; 32],
    mps: [u32; 32],
    tds: [Option<Td>; up::RANURAS],
    orden: u64,
    config: u8,
    largo_config: usize,
    alt: [u8; 32],
    hechas: u64,
    canceladas: u64,
}

/// Lo del préstamo: el dominio que pide (su llave), lo prestado y lo que espera para salir.
struct Prestamos {
    dominio: Option<u32>,
    /// La llave para que el dominio escriba al canal propio (se le da en cada `HOLA`).
    para_dominio: u32,
    prestados: [Option<Prestado>; MAX_PRESTADOS],
    salida: [[u8; up::MAX_MENSAJE]; SALIDA],
    s_ini: usize,
    s_n: usize,
    /// El dominio se fue (su canal se cerró): hay que devolver lo suyo.
    perdido: bool,
    rechazos: u64,
}

impl Prestamos {
    fn enviar(&mut self, m: up::Mensaje) {
        let mut b = [0u8; up::MAX_MENSAJE];
        let n = m.escribir(&mut b);
        let Some(d) = self.dominio else { return };
        if self.s_n == 0 {
            let r = dashos::canal_escribir(d, &b[..n], &[]);
            if r == 0 {
                return;
            }
            if r == dashos::E_CERRADO {
                self.perdido = true;
                return;
            }
        }
        if self.s_n == SALIDA {
            decir!("MAL: la cola de mensajes al dominio está llena; uno se pierde");
            return;
        }
        self.salida[(self.s_ini + self.s_n) % SALIDA] = b;
        self.s_n += 1;
    }

    /// Lo que esperaba sitio, en orden, mientras quepa.
    fn reintentar(&mut self) {
        let Some(d) = self.dominio else {
            self.s_n = 0;
            return;
        };
        while self.s_n > 0 {
            let b = self.salida[self.s_ini];
            let n = up::Mensaje::leer(&b).map_or(up::MAX_MENSAJE, |m| m.escribir(&mut [0; up::MAX_MENSAJE]));
            let r = dashos::canal_escribir(d, &b[..n], &[]);
            if r == dashos::E_CERRADO {
                self.perdido = true;
                return;
            }
            if r != 0 {
                return;
            }
            self.s_ini = (self.s_ini + 1) % SALIDA;
            self.s_n -= 1;
        }
    }

    fn hecho(&mut self, aparato: u32, etiqueta: u32, estado: i32, hecho: u32) {
        self.enviar(up::Mensaje::Hecho { aparato, etiqueta, estado, hecho });
    }
}

/// Se presta: ni concentradores ni lo que usa el propio xHCI (teclados, ratones y tabletas).
fn se_presta(r: &Ranura) -> bool {
    !r.hub && !r.activo
}

fn ofrecer(pm: &mut Prestamos, r: &Ranura) {
    if pm.dominio.is_none() || !se_presta(r) {
        return;
    }
    let datos = up::Aparato { velocidad: r.velocidad as u8, puerto: r.puerto as u8, ruta: r.ruta, vid: r.vid, pid: r.pid, clase: r.clase, interfaz: r.interfaz };
    decir!("ofrece al dominio el aparato {} ({:04x}:{:04x}, interfaz {}/{}/{})", r.aparato, r.vid, r.pid, r.interfaz.0, r.interfaz.1, r.interfaz.2);
    pm.enviar(up::Mensaje::Aparato { aparato: r.aparato, datos });
}

/// El contexto de salida de una ranura: el estado de un punto (6.2.3: 1 en marcha, 2 atascado, 3 parado) y su TR
/// Dequeue Pointer.
fn estado_punto(x: &Xhci, k: usize, dci: u32) -> (u32, u64) {
    let off = base_ranura(k) + R_SALIDA + dci as usize * x.ctx;
    // SAFETY: el contexto de salida de la ranura `k`, dentro de la memoria DMA del controlador.
    let w = |i: usize| u32::from_le(unsafe { core::ptr::read_volatile((VA_DMA + off + 4 * i) as *const u32) });
    (w(0) & 7, (w(2) as u64 | (w(3) as u64) << 32))
}

/// El anillo de un punto del prestado (el 0, el de su ranura).
fn anillo<'a>(pr: &'a mut Prestado, lista: &'a mut [Option<Ranura>; MAX_RANURAS], dci: u32) -> Option<&'a mut Anillo> {
    if dci == 1 {
        lista[pr.k].as_mut().map(|r| &mut r.ep0)
    } else {
        pr.anillos.get_mut(dci as usize)?.as_mut()
    }
}

/// Deja un punto atascado (o parado) listo otra vez (4.6.8, 4.6.10): Reset Endpoint si está atascado, y el TR Dequeue
/// en la siguiente transferencia que quede (o donde irá la próxima). Con lo que quede, el timbre.
fn desatascar(x: &mut Xhci, pr: &mut Prestado, lista: &mut [Option<Ranura>; MAX_RANURAS], dci: u32) {
    let Some(id) = lista[pr.k].map(|r| r.id) else { return };
    if estado_punto(x, pr.k, dci).0 == 2 {
        let _ = x.orden(0, (T_REINICIAR_PUNTO << 10) | (dci << 16) | (id << 24));
    }
    let siguiente = pr.tds.iter().flatten().filter(|t| t.dci == dci).min_by_key(|t| t.orden).map(|t| (t.trbs[0], t.ciclo0));
    let Some(a) = anillo(pr, lista, dci) else { return };
    let (b, c) = siguiente.unwrap_or_else(|| a.siguiente());
    let _ = x.orden(b | c as u64, (T_PONER_DEQUEUE << 10) | (dci << 16) | (id << 24));
    if siguiente.is_some() {
        x.timbre(id as usize, dci);
    }
}

/// Un evento de transferencia de un aparato prestado.
fn evento_prestado(x: &mut Xhci, pm: &mut Prestamos, i: usize, lista: &mut [Option<Ranura>; MAX_RANURAS], ev: Evento) {
    let Some(pr) = pm.prestados[i].as_mut() else { return };
    let (dci, codigo, resto, trb) = ((ev.2 >> 16) & 0x1f, ev.1 >> 24, ev.1 & 0xff_ffff, ev.0);
    let Some(j) = pr.tds.iter().position(|t| t.is_some_and(|t| t.dci == dci && t.trbs[..t.n].contains(&trb))) else { return };
    let mut td = pr.tds[j].unwrap();
    let estado = match codigo {
        1 | 13 => {
            if trb != td.ultimo {
                // La etapa de datos de una de control (o la primera parte de una con paquete vacío detrás).
                td.hecho = Some(td.largo.saturating_sub(resto));
                pr.tds[j] = Some(td);
                return;
            }
            up::BIEN
        }
        // Parada (al cancelar): lo decide `cancelar`.
        26..=28 => return,
        6 => up::ATASCADO,
        3 => up::DESBORDE,
        _ => up::ERROR,
    };
    let hecho = if trb == td.datos && td.datos == td.ultimo { td.largo.saturating_sub(resto) } else { td.hecho.unwrap_or(if estado == up::BIEN { td.largo } else { 0 }) };
    pr.tds[j] = None;
    pr.hechas += 1;
    let aparato = pr.aparato;
    if estado != up::BIEN {
        desatascar(x, pr, lista, dci);
    }
    pm.hecho(aparato, td.etiqueta, estado, hecho.min(td.largo));
}

/// Pone una transferencia del prestado en su anillo y toca el timbre.
fn encolar(x: &mut Xhci, pr: &mut Prestado, lista: &mut [Option<Ranura>; MAX_RANURAS], m: up::Mensaje) -> Result<(), i32> {
    let Some(r) = lista[pr.k] else { return Err(up::SIN_APARATO) };
    let Some(libre) = pr.tds.iter().position(|t| t.is_none()) else { return Err(up::NO_VALE) };
    let buferes = pr.buferes_bus;
    let orden = pr.orden;
    let td = match m {
        up::Mensaje::Control { etiqueta, ranura, setup, largo, .. } => {
            if !up::control_valido(ranura, &setup, largo) || pr.tds.iter().flatten().any(|t| t.dci == 1) {
                return Err(up::NO_VALE);
            }
            let entrada = setup[0] & 0x80 != 0;
            let datos = buferes + ranura as u64 * up::MAX_DATOS as u64;
            let a = anillo(pr, lista, 1).ok_or(up::SIN_APARATO)?;
            let (_, ciclo0) = a.siguiente();
            let tt = if largo == 0 { 0 } else if entrada { 3 } else { 2 };
            let t0 = a.poner(u64::from_le_bytes(setup), 8, (T_SETUP << 10) | (1 << 6) | (tt << 16));
            let td_datos = if largo > 0 { a.poner(datos, largo, (T_DATOS << 10) | TRB_ISP | if entrada { 1 << 16 } else { 0 }) } else { 0 };
            // El estado, al revés que los datos (de entrada si no hay datos o son de salida).
            let t2 = a.poner(0, 0, (T_ESTADO << 10) | TRB_IOC | if largo > 0 && entrada { 0 } else { 1 << 16 });
            let trbs = if largo > 0 { [t0, td_datos, t2, 0] } else { [t0, t2, 0, 0] };
            Td { etiqueta, dci: 1, largo, trbs, n: if largo > 0 { 3 } else { 2 }, ultimo: t2, datos: td_datos, ciclo0, hecho: None, orden }
        }
        up::Mensaje::Transferir { etiqueta, ranura, punto, cero, largo, .. } => {
            if !up::transferir_valido(ranura, punto, largo) {
                return Err(up::NO_VALE);
            }
            let dci = 2 * (punto & 0xf) as u32 + (punto >> 7) as u32;
            let tipo = pr.tipos[dci as usize];
            let mps = pr.mps[dci as usize].max(1);
            // El punto tiene que estar configurado, y en su dirección.
            if tipo == 0 || (tipo >= 5) != (punto & 0x80 != 0) || pr.tds.iter().flatten().filter(|t| t.dci == dci).count() >= 4 {
                return Err(up::NO_VALE);
            }
            let datos = buferes + ranura as u64 * up::MAX_DATOS as u64;
            let a = anillo(pr, lista, dci).ok_or(up::NO_VALE)?;
            let (_, ciclo0) = a.siguiente();
            let t0 = a.poner(datos, largo, (T_NORMAL << 10) | TRB_ISP | TRB_IOC);
            // Un paquete vacío detrás (URB_ZERO_PACKET), en su propio TD (4.10.1.1).
            let zlp = cero && largo > 0 && largo % mps == 0 && punto & 0x80 == 0;
            let t1 = if zlp { a.poner(0, 0, (T_NORMAL << 10) | TRB_IOC) } else { t0 };
            Td { etiqueta, dci, largo, trbs: [t0, t1, 0, 0], n: if zlp { 2 } else { 1 }, ultimo: t1, datos: t0, ciclo0, hecho: None, orden }
        }
        _ => return Err(up::NO_VALE),
    };
    pr.orden += 1;
    pr.tds[libre] = Some(td);
    x.timbre(r.id as usize, td.dci);
    Ok(())
}

/// Cancela la transferencia `etiqueta` (4.6.9): para el punto, atiende lo que ya hubiera acabado, y si sigue en
/// marcha convierte sus TRB en "no hacer nada" (6.4.1.4), mueve el TR Dequeue a donde se paró y sigue.
fn cancelar(x: &mut Xhci, pm: &mut Prestamos, i: usize, lista: &mut [Option<Ranura>; MAX_RANURAS], etiqueta: u32) {
    let Some(pr) = pm.prestados[i].as_mut() else { return };
    let Some(td) = pr.tds.iter().flatten().find(|t| t.etiqueta == etiqueta).copied() else { return };
    let Some(id) = lista[pr.k].map(|r| r.id) else { return };
    let _ = x.orden(0, (T_PARAR_PUNTO << 10) | (td.dci << 16) | (id << 24));
    // Lo que acabó antes de pararse (sus eventos están guardados): se contesta como acabado.
    for _ in 0..x.g_n {
        let Some(ev) = x.siguiente() else { break };
        let (ranura, dci) = (ev.2 >> 24, (ev.2 >> 16) & 0x1f);
        if tipo_de(&ev) == T_EV_TRANSFERENCIA && ranura == id && dci == td.dci {
            evento_prestado(x, pm, i, lista, ev);
        } else {
            x.guardar(ev);
        }
    }
    let Some(pr) = pm.prestados[i].as_mut() else { return };
    let Some(j) = pr.tds.iter().position(|t| t.is_some_and(|t| t.etiqueta == etiqueta)) else {
        // Acabó antes: su HECHO ya salió. El punto, otra vez en marcha.
        x.timbre(id as usize, td.dci);
        return;
    };
    let aparato = pr.aparato;
    if let Some(a) = anillo(pr, lista, td.dci) {
        for &b in &td.trbs[..td.n] {
            if let Some(va) = a.va_de(b) {
                poner_trb(va, 0, 0, (T_NOOP << 10) | (leer_trb_control(va) & 1));
            }
        }
    }
    let pr = pm.prestados[i].as_mut().unwrap();
    pr.tds[j] = None;
    pr.canceladas += 1;
    let (estado, deq) = estado_punto(x, pr.k, td.dci);
    if estado == 2 {
        desatascar(x, pr, lista, td.dci);
    } else {
        let _ = x.orden(deq & !0xe, (T_PONER_DEQUEUE << 10) | (td.dci << 16) | (id << 24));
        x.timbre(id as usize, td.dci);
    }
    pm.hecho(aparato, etiqueta, up::CANCELADO, 0);
}

/// Los puntos de una interfaz y alternativa del descriptor de configuración `d`: (dirección, atributos, wMaxPacketSize,
/// bInterval). `ifz` = None: los de la alternativa 0 de todas.
fn puntos(d: &[u8], ifz: Option<u8>, alt: u8) -> ([(u8, u8, u16, u8); 30], usize) {
    let mut v = [(0u8, 0u8, 0u16, 0u8); 30];
    let mut n = 0;
    let (mut i, mut dentro) = (0usize, false);
    while i + 1 < d.len() && d[i] >= 2 {
        match d[i + 1] {
            4 if i + 3 < d.len() => dentro = ifz.is_none_or(|f| f == d[i + 2]) && d[i + 3] == alt,
            5 if i + 6 < d.len() && dentro && n < v.len() => {
                v[n] = (d[i + 2], d[i + 3], u16::from_le_bytes([d[i + 4], d[i + 5]]), d[i + 6]);
                n += 1;
            }
            _ => {}
        }
        i += d[i] as usize;
    }
    (v, n)
}

fn dci_de(direccion: u8) -> u32 {
    2 * (direccion & 0xf) as u32 + (direccion >> 7) as u32
}

/// Configure Endpoint (4.6.6) del prestado: quita los puntos de `quitar` (una máscara de DCI) y pone los de `poner`,
/// con anillos nuevos. Las isócronas y los de control que no son el 0 no se ponen.
fn configurar_puntos(x: &mut Xhci, pr: &mut Prestado, lista: &[Option<Ranura>; MAX_RANURAS], quitar: u32, poner: &[(u8, u8, u16, u8)]) -> bool {
    let Some(r) = lista[pr.k] else { return false };
    let (c, e) = (x.ctx, base_ranura(pr.k) + R_ENTRADA);
    for i in (0..33 * c).step_by(4) {
        x.poner32(e + i, 0);
    }
    let mut añadir = 1u32;
    let mut quedan = 0u32;
    for dci in 2..32 {
        if pr.tipos[dci] != 0 && quitar & (1 << dci) == 0 {
            quedan |= 1 << dci;
        }
    }
    for &(dir, attr, wmps, intervalo_b) in poner {
        let tipo_usb = attr & 3;
        if tipo_usb == 0 || tipo_usb == 1 || dir & 0xf == 0 {
            continue;
        }
        let entrada = dir & 0x80 != 0;
        let dci = dci_de(dir);
        let tipo = match (tipo_usb, entrada) {
            (2, false) => 2,
            (3, false) => 3,
            (2, true) => 6,
            _ => 7,
        };
        let mps = (wmps & 0x7ff) as u32;
        let rafaga = if r.velocidad == 3 && tipo_usb == 3 { ((wmps >> 11) & 3) as u32 } else { 0 };
        let intervalo = if tipo_usb != 3 {
            0
        } else if r.velocidad >= 3 {
            (intervalo_b as u32).clamp(1, 16) - 1
        } else {
            (31 - ((intervalo_b as u32).max(1) * 8).leading_zeros()).clamp(3, 10)
        };
        let va = VA_PRESTADO + ENTRE_PRESTADOS * pr.indice + 1024 * dci as usize;
        let a = Anillo::en(va, pr.privada_bus + 1024 * dci as u64);
        let ep = e + (dci as usize + 1) * c;
        x.poner32(ep, intervalo << 16);
        x.poner32(ep + 4, (3 << 1) | (tipo << 3) | (rafaga << 8) | (mps << 16));
        x.poner64(ep + 8, a.bus | 1);
        let carga = mps * (rafaga + 1);
        x.poner32(ep + 16, if tipo_usb == 2 { 3072 } else { carga | (carga << 16) });
        pr.anillos[dci as usize] = Some(a);
        pr.tipos[dci as usize] = tipo as u8;
        pr.mps[dci as usize] = mps;
        añadir |= 1 << dci;
        quedan |= 1 << dci;
    }
    let ultimo = 31 - (quedan | 2).leading_zeros();
    x.poner32(e, quitar & !3);
    x.poner32(e + 4, añadir);
    x.poner32(e + c, (r.velocidad << 20) | (ultimo << 27) | r.ruta);
    x.poner32(e + c + 4, r.puerto << 16);
    x.poner32(e + c + 8, r.tt.map_or(0, |(ranura, puerto)| ranura | puerto << 8));
    let bien = matches!(x.orden(x.bus + e as u64, (T_CONFIGURAR_EP << 10) | (r.id << 24)), Some((1, _)));
    for dci in 2..32 {
        if quitar & (1 << dci) != 0 && añadir & (1 << dci) == 0 {
            pr.anillos[dci] = None;
            pr.tipos[dci] = 0;
            pr.mps[dci] = 0;
        }
    }
    bien
}

/// Una de control que es de la pila USB y no se pasa tal cual (USB 2.0, 9.4): `SET_ADDRESS` (la dirección la lleva el
/// xHCI), `SET_CONFIGURATION` y `SET_INTERFACE` (cambian los puntos que hay que configurar en el controlador) y
/// `CLEAR_FEATURE(ENDPOINT_HALT)` (el punto se reinicia también aquí). Devuelve su estado, o nada si no es de esas.
fn control_de_la_pila(x: &mut Xhci, pm: &mut Prestamos, i: usize, lista: &mut [Option<Ranura>; MAX_RANURAS], setup: [u8; 8]) -> Option<i32> {
    let valor = u16::from_le_bytes([setup[2], setup[3]]);
    let indice = u16::from_le_bytes([setup[4], setup[5]]);
    let es = match (setup[0], setup[1]) {
        (0x00, 5) => 1,
        (0x00, 9) => 2,
        (0x01, 11) => 3,
        (0x02, 1) if valor == 0 => 4,
        _ => return None,
    };
    let pr = pm.prestados[i].as_mut()?;
    let k = pr.k;
    let mut r = lista[k]?;
    // Lo que estuviera en marcha en los puntos que se tocan se da por cancelado.
    let cancelar_dci = |pm: &mut Prestamos, mascara: u32| {
        let pr = pm.prestados[i].as_mut().unwrap();
        let aparato = pr.aparato;
        let mut fuera = [0u32; up::RANURAS];
        let mut n = 0;
        for t in pr.tds.iter_mut() {
            if t.is_some_and(|t| mascara & (1 << t.dci) != 0) {
                fuera[n] = t.unwrap().etiqueta;
                n += 1;
                *t = None;
            }
        }
        for &e in &fuera[..n] {
            pm.hecho(aparato, e, up::CANCELADO, 0);
        }
    };
    // Lo que sí va al aparato, por el punto 0 (sin datos); si se atasca, se desatasca.
    let al_aparato = |x: &mut Xhci, pm: &mut Prestamos, lista: &mut [Option<Ranura>; MAX_RANURAS], r: &mut Ranura| -> bool {
        let bien = x.control_en(r, setup, 0, 0).is_some();
        lista[k] = Some(*r);
        if !bien {
            let pr = pm.prestados[i].as_mut().unwrap();
            desatascar(x, pr, lista, 1);
        }
        bien
    };
    match es {
        // SET_ADDRESS: el aparato ya tiene la suya.
        1 => Some(up::BIEN),
        2 => {
            let v = valor as u8;
            let mut todos = 0u32;
            for dci in 2..32 {
                if pr.tipos[dci] != 0 {
                    todos |= 1 << dci;
                }
            }
            // La configuración tiene que existir: se busca su descriptor (entero, en la memoria propia del préstamo).
            let mut largo = 0usize;
            if v != 0 {
                let base = VA_PRESTADO + ENTRE_PRESTADOS * pr.indice + P_CONFIG;
                let bus = pr.privada_bus + P_CONFIG as u64;
                for indice in 0..r.configuraciones.min(8) {
                    let Some(_) = x.control_en(&mut r, [0x80, 6, indice, 2, 0, 0, 9, 0], 9, bus) else { break };
                    // SAFETY: lo que acaba de escribir el controlador en la memoria propia del préstamo.
                    let cab: [u8; 9] = core::array::from_fn(|j| unsafe { core::ptr::read_volatile((base + j) as *const u8) });
                    if cab[5] != v {
                        continue;
                    }
                    let total = (u16::from_le_bytes([cab[2], cab[3]]) as usize).clamp(9, MAX_CONFIG);
                    if x.control_en(&mut r, [0x80, 6, indice, 2, 0, 0, total as u8, (total >> 8) as u8], total as u32, bus).is_some() {
                        largo = total;
                    }
                    break;
                }
                lista[k] = Some(r);
                if largo == 0 {
                    decir!("el dominio pide la configuración {} del aparato {}, que no existe", v, pr.aparato);
                    return Some(up::NO_VALE);
                }
            }
            cancelar_dci(pm, todos);
            if !al_aparato(x, pm, lista, &mut r) {
                return Some(up::ATASCADO);
            }
            let pr = pm.prestados[i].as_mut().unwrap();
            let base = VA_PRESTADO + ENTRE_PRESTADOS * pr.indice + P_CONFIG;
            // SAFETY: el descriptor que se acaba de leer, `largo` bytes en la memoria propia del préstamo.
            let d: [u8; MAX_CONFIG] = core::array::from_fn(|j| if j < largo { unsafe { core::ptr::read_volatile((base + j) as *const u8) } } else { 0 });
            let (v_p, n) = if v != 0 { puntos(&d[..largo], None, 0) } else { ([(0, 0, 0, 0); 30], 0) };
            pr.config = v;
            pr.largo_config = largo;
            pr.alt = [0; 32];
            let bien = configurar_puntos(x, pr, lista, todos, &v_p[..n]);
            decir!(
                "aparato {}: configuración {} ({} puntos{})",
                pr.aparato, v, n, if bien { "" } else { "; el controlador no los acepta" }
            );
            Some(if bien { up::BIEN } else { up::NO_PUEDE })
        }
        3 => {
            let (ifz, alt) = (indice as u8, valor as u8);
            if pr.config == 0 || ifz as usize >= pr.alt.len() {
                return Some(up::NO_VALE);
            }
            let base = VA_PRESTADO + ENTRE_PRESTADOS * pr.indice + P_CONFIG;
            let largo = pr.largo_config;
            // SAFETY: el descriptor de la configuración en uso, en la memoria propia del préstamo.
            let d: [u8; MAX_CONFIG] = core::array::from_fn(|j| if j < largo { unsafe { core::ptr::read_volatile((base + j) as *const u8) } } else { 0 });
            let d = &d[..largo];
            // Que exista esa alternativa de esa interfaz.
            let mut existe = false;
            let mut j = 0;
            while j + 3 < d.len() && d[j] >= 2 {
                existe |= d[j + 1] == 4 && d[j + 2] == ifz && d[j + 3] == alt;
                j += d[j] as usize;
            }
            if !existe {
                return Some(up::NO_VALE);
            }
            let (viejos, nv) = puntos(d, Some(ifz), pr.alt[ifz as usize]);
            let (nuevos, nn) = puntos(d, Some(ifz), alt);
            let quitar = viejos[..nv].iter().fold(0u32, |m, p| m | 1 << dci_de(p.0));
            cancelar_dci(pm, quitar);
            if !al_aparato(x, pm, lista, &mut r) {
                return Some(up::ATASCADO);
            }
            let pr = pm.prestados[i].as_mut().unwrap();
            pr.alt[ifz as usize] = alt;
            let bien = configurar_puntos(x, pr, lista, quitar, &nuevos[..nn]);
            decir!("aparato {}: interfaz {}, alternativa {} ({} puntos)", pr.aparato, ifz, alt, nn);
            Some(if bien { up::BIEN } else { up::NO_PUEDE })
        }
        _ => {
            // CLEAR_FEATURE(ENDPOINT_HALT): al aparato, y aquí el punto de nuevo (el conmutador de datos a cero).
            let dir = indice as u8;
            let dci = dci_de(dir);
            if dir & 0x70 != 0 || (dir & 0xf != 0 && pr.tipos[dci as usize] == 0) {
                return Some(up::NO_VALE);
            }
            if dir & 0xf != 0 {
                cancelar_dci(pm, 1 << dci);
            }
            if !al_aparato(x, pm, lista, &mut r) {
                return Some(up::ATASCADO);
            }
            if dir & 0xf == 0 {
                return Some(up::BIEN);
            }
            let pr = pm.prestados[i].as_mut().unwrap();
            let p = (dir, if pr.tipos[dci as usize] & 3 == 3 { 3 } else { 2 }, pr.mps[dci as usize] as u16, 1);
            // El intervalo de uno de interrupción, del descriptor.
            let base = VA_PRESTADO + ENTRE_PRESTADOS * pr.indice + P_CONFIG;
            let largo = pr.largo_config;
            // SAFETY: como arriba.
            let d: [u8; MAX_CONFIG] = core::array::from_fn(|j| if j < largo { unsafe { core::ptr::read_volatile((base + j) as *const u8) } } else { 0 });
            let (todos, n) = puntos(&d[..largo], None, 0);
            let p = todos[..n].iter().copied().find(|q| q.0 == dir).unwrap_or(p);
            let bien = configurar_puntos(x, pr, lista, 1 << dci, &[p]);
            Some(if bien { up::BIEN } else { up::NO_PUEDE })
        }
    }
}

/// Toma un aparato para el dominio: su memoria propia (anillos) y los búferes, que se le prestan.
fn tomar(pm: &mut Prestamos, lista: &[Option<Ranura>; MAX_RANURAS], aparato: u32) -> Result<(usize, u32), i32> {
    let Some(k) = lista.iter().position(|r| r.is_some_and(|r| r.aparato == aparato)) else { return Err(up::SIN_APARATO) };
    let r = lista[k].unwrap();
    if !se_presta(&r) {
        return Err(up::NO_VALE);
    }
    if pm.prestados.iter().flatten().any(|p| p.aparato == aparato) {
        return Err(up::OCUPADO);
    }
    let Some(indice) = pm.prestados.iter().position(|p| p.is_none()) else { return Err(up::NO_PUEDE) };
    // Lo de préstamos anteriores ya no lo toca el controlador (sus ranuras se desactivaron): vuelve.
    dashos::dma_liberar(1);
    let (privada, privada_bus) = dashos::memoria_dma(1, TAM_PRIVADA);
    let (buferes, buferes_bus) = dashos::memoria_dma(1, up::TAM_BUFERES);
    let va_p = VA_PRESTADO + ENTRE_PRESTADOS * indice;
    let va_b = VA_BUFERES + ENTRE_PRESTADOS * indice;
    let bien = privada > 0 && buferes > 0 && dashos::memoria_proyectar(privada as u32, va_p, true) == 0 && dashos::memoria_proyectar(buferes as u32, va_b, true) == 0;
    if !bien {
        for (l, va) in [(privada, va_p), (buferes, va_b)] {
            if l > 0 {
                dashos::memoria_quitar(va);
                dashos::cerrar(l as u32);
            }
        }
        return Err(up::NO_PUEDE);
    }
    let copia = dashos::llave_duplicar(buferes as u32, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, 0);
    if copia <= 0 {
        return Err(up::NO_PUEDE);
    }
    pm.prestados[indice] = Some(Prestado {
        aparato,
        indice,
        k,
        privada: privada as u32,
        privada_bus,
        buferes: buferes as u32,
        buferes_bus,
        anillos: [None; 32],
        tipos: [0; 32],
        mps: [0; 32],
        tds: [None; up::RANURAS],
        orden: 0,
        config: 0,
        largo_config: 0,
        alt: [0; 32],
        hechas: 0,
        canceladas: 0,
    });
    decir!("presta al dominio el aparato {} ({:04x}:{:04x}, ranura {}): {} búferes de {} KB", aparato, r.vid, r.pid, r.id, up::RANURAS, up::MAX_DATOS / 1024);
    Ok((indice, copia as u32))
}

/// Desactiva la ranura de `k` (4.6.4): el controlador deja de tocar sus anillos y su memoria.
fn desactivar(x: &mut Xhci, lista: &mut [Option<Ranura>; MAX_RANURAS], k: usize) {
    if let Some(r) = lista[k] {
        let _ = x.orden(0, (T_DESACTIVAR_RANURA << 10) | (r.id << 24));
        x.poner64(M_DCBAA + 8 * r.id as usize, 0);
    }
}

/// Acaba el préstamo `i`: la ranura desactivada (lo que tuviera en marcha ya no se hace) y su memoria suelta. Con
/// `volver`, el aparato vuelve a DashOS como nuevo: se reinicia su puerto y se configura otra vez (otro número), y se
/// ofrece de nuevo.
fn acabar_prestamo(x: &mut Xhci, op: usize, pm: &mut Prestamos, lista: &mut [Option<Ranura>; MAX_RANURAS], i: usize, volver: bool, por: &str) {
    let Some(pr) = pm.prestados[i].take() else { return };
    let en_marcha = pr.tds.iter().flatten().count();
    desactivar(x, lista, pr.k);
    for va in [VA_PRESTADO + ENTRE_PRESTADOS * pr.indice, VA_BUFERES + ENTRE_PRESTADOS * pr.indice] {
        dashos::memoria_quitar(va);
    }
    dashos::cerrar(pr.privada);
    dashos::cerrar(pr.buferes);
    let antes = lista[pr.k].take();
    if !volver {
        decir!("aparato {}: el préstamo acaba ({}): {} transferencias hechas, {} canceladas, {} sin acabar", pr.aparato, por, pr.hechas, pr.canceladas, en_marcha);
        return;
    }
    let Some(r) = antes else { return };
    match reenumerar(x, op, lista, pr.k, &r) {
        Some(nuevo) => {
            decir!(
                "aparato {} devuelto ({}): {} transferencias hechas, {} canceladas, {} sin acabar; ranura {} desactivada, puerto reiniciado: vuelve limpio como aparato {} (ranura {})",
                pr.aparato, por, pr.hechas, pr.canceladas, en_marcha, r.id, nuevo.aparato, nuevo.id
            );
            ofrecer(pm, &nuevo);
        }
        None => decir!("aparato {} devuelto ({}), pero no se pudo volver a configurar (en {}: código {})", pr.aparato, por, x.paso, x.codigo),
    }
}

/// Reinicia el puerto de `r` (el raíz, o el de su concentrador) y lo configura de nuevo en el sitio `k`.
fn reenumerar(x: &mut Xhci, op: usize, lista: &mut [Option<Ranura>; MAX_RANURAS], k: usize, r: &Ranura) -> Option<Ranura> {
    let velocidad = match r.padre {
        None => {
            let reg = op + PORTSC + 0x10 * (r.puerto as usize - 1);
            if leer(reg) & PORT_CCS == 0 {
                return None;
            }
            escribir(reg, (leer(reg) & !PORT_RW1C) | PORT_PR);
            if !esperar(200, || leer(reg) & PORT_PRC != 0) || leer(reg) & PORT_PED == 0 {
                return None;
            }
            escribir(reg, (leer(reg) & !PORT_RW1C) | PORT_PRC);
            (leer(reg) >> 10) & 0xf
        }
        Some((kh, puerto)) => {
            let mut h = lista[kh]?;
            let _ = x.control(&mut h, kh, [0x23, 3, 4, 0, puerto as u8, 0, 0, 0], 0); // SET_FEATURE(PORT_RESET)
            let mut listo = None;
            for _ in 0..20 {
                dashos::dormir_us(10_000);
                if x.control(&mut h, kh, [0xA3, 0, 0, 0, puerto as u8, 0, 4, 0], 4).is_some() {
                    let d = x.datos(kh, 4);
                    let (e, cambio) = (u16::from_le_bytes([d[0], d[1]]), u16::from_le_bytes([d[2], d[3]]));
                    if cambio & (1 << 4) != 0 && e & 2 != 0 {
                        listo = Some(e);
                        break;
                    }
                }
            }
            let _ = x.control(&mut h, kh, [0x23, 1, 20, 0, puerto as u8, 0, 0, 0], 0); // CLEAR_FEATURE(C_PORT_RESET)
            lista[kh] = Some(h);
            let e = listo?;
            if e & (1 << 9) != 0 { 2 } else if e & (1 << 10) != 0 { 3 } else { 1 }
        }
    };
    let mut n = configurar(x, r.puerto, velocidad, k, r.ruta, r.tt)?;
    n.padre = r.padre;
    lista[k] = Some(n);
    Some(n)
}

/// Un cambio en un puerto raíz (4.19.2): lo que se desenchufó se quita (si estaba prestado, el dominio lo pierde) y lo
/// que se enchufa en un puerto vacío se configura (y se ofrece). Un concentrador enchufado en marcha no se recorre.
fn puerto_cambia(x: &mut Xhci, op: usize, pm: &mut Prestamos, lista: &mut [Option<Ranura>; MAX_RANURAS], puerto: u32, max: usize) {
    if puerto == 0 {
        return;
    }
    let reg = op + PORTSC + 0x10 * (puerto as usize - 1);
    let sc = leer(reg);
    escribir(reg, (sc & !PORT_RW1C) | (sc & PORT_CAMBIOS));
    let hay = lista.iter().any(|r| r.is_some_and(|r| r.puerto == puerto));
    if sc & PORT_CCS == 0 && hay {
        for k in 0..MAX_RANURAS {
            let Some(r) = lista[k].filter(|r| r.puerto == puerto) else { continue };
            if let Some(i) = pm.prestados.iter().position(|p| p.as_ref().is_some_and(|p| p.k == k)) {
                decir!("puerto {}: se desenchufa el aparato {} ({:04x}:{:04x}), prestado: el dominio lo pierde", puerto, r.aparato, r.vid, r.pid);
                pm.enviar(up::Mensaje::Quitado { aparato: r.aparato });
                acabar_prestamo(x, op, pm, lista, i, false, "se desenchufó");
            } else {
                decir!("puerto {}: se desenchufa el aparato {} ({:04x}:{:04x})", puerto, r.aparato, r.vid, r.pid);
                desactivar(x, lista, k);
                lista[k] = None;
            }
        }
    } else if sc & PORT_CCS != 0 && !hay {
        let Some(k) = (0..max.min(MAX_RANURAS)).find(|&k| lista[k].is_none()) else {
            decir!("puerto {}: algo se enchufa, pero no quedan ranuras", puerto);
            return;
        };
        dashos::dormir_us(100_000);
        escribir(reg, (leer(reg) & !PORT_RW1C) | PORT_PR);
        if !esperar(200, || leer(reg) & PORT_PRC != 0) || leer(reg) & PORT_PED == 0 {
            decir!("puerto {}: algo se enchufa, pero no se activa (PORTSC {:#x})", puerto, leer(reg));
            return;
        }
        escribir(reg, (leer(reg) & !PORT_RW1C) | PORT_PRC);
        let velocidad = (leer(reg) >> 10) & 0xf;
        match configurar(x, puerto, velocidad, k, 0, None) {
            Some(r) => {
                lista[k] = Some(r);
                decir!("puerto {}: se enchufa: ranura {}, {}, {}", puerto, r.id, que_es(&r), Aparato(&r));
                if r.hub {
                    decir!("puerto {}: un concentrador enchufado en marcha: sus puertos no se miran", puerto);
                }
                ofrecer(pm, &r);
            }
            None => decir!("puerto {}: se enchufa algo que no se pudo configurar (en {}: código {})", puerto, x.paso, x.codigo),
        }
    }
}

/// Lo que pide el dominio (ya conocido) por el canal propio.
fn atender_dominio(x: &mut Xhci, op: usize, pm: &mut Prestamos, lista: &mut [Option<Ranura>; MAX_RANURAS], b: &[u8]) {
    let Some(m) = up::Mensaje::leer(b) else {
        pm.rechazos += 1;
        return;
    };
    let indice = |pm: &Prestamos, aparato: u32| pm.prestados.iter().position(|p| p.as_ref().is_some_and(|p| p.aparato == aparato));
    match m {
        up::Mensaje::Tomar { aparato } => match tomar(pm, lista, aparato) {
            Ok((_, copia)) => {
                let mut b = [0u8; up::MAX_MENSAJE];
                let n = up::Mensaje::Tomado { aparato, estado: up::BIEN }.escribir(&mut b);
                if let Some(d) = pm.dominio {
                    if dashos::canal_escribir(d, &b[..n], &[copia]) != 0 {
                        dashos::cerrar(copia);
                        if let Some(i) = indice(pm, aparato) {
                            acabar_prestamo(x, op, pm, lista, i, true, "el dominio no recibe los búferes");
                        }
                    }
                }
            }
            Err(e) => {
                pm.rechazos += 1;
                let que = lista.iter().flatten().find(|r| r.aparato == aparato).map_or("no existe", |r| if r.hub { "es un concentrador" } else if r.activo { "lo usa DashOS (teclado, ratón o tableta)" } else { "ya está prestado" });
                decir!("el dominio pide el aparato {}: no se presta ({}; estado {})", aparato, que, e);
                pm.enviar(up::Mensaje::Tomado { aparato, estado: e });
            }
        },
        up::Mensaje::Soltar { aparato } => {
            let estado = match indice(pm, aparato) {
                Some(i) => {
                    acabar_prestamo(x, op, pm, lista, i, true, "lo devuelve el dominio");
                    up::BIEN
                }
                None => up::SIN_APARATO,
            };
            pm.enviar(up::Mensaje::Soltado { aparato, estado });
        }
        up::Mensaje::Control { aparato, etiqueta, .. } | up::Mensaje::Transferir { aparato, etiqueta, .. } if indice(pm, aparato).is_none() => {
            pm.hecho(aparato, etiqueta, up::SIN_APARATO, 0);
        }
        up::Mensaje::Control { aparato, etiqueta, setup, ranura, largo } => {
            let i = indice(pm, aparato).unwrap();
            if !up::control_valido(ranura, &setup, largo) {
                pm.hecho(aparato, etiqueta, up::NO_VALE, 0);
                return;
            }
            if let Some(estado) = control_de_la_pila(x, pm, i, lista, setup) {
                pm.hecho(aparato, etiqueta, estado, 0);
                return;
            }
            let pr = pm.prestados[i].as_mut().unwrap();
            if let Err(e) = encolar(x, pr, lista, m) {
                pm.hecho(aparato, etiqueta, e, 0);
            }
        }
        up::Mensaje::Transferir { aparato, etiqueta, .. } => {
            let i = indice(pm, aparato).unwrap();
            let pr = pm.prestados[i].as_mut().unwrap();
            if let Err(e) = encolar(x, pr, lista, m) {
                pm.hecho(aparato, etiqueta, e, 0);
            }
        }
        up::Mensaje::Cancelar { aparato, etiqueta } => {
            if let Some(i) = indice(pm, aparato) {
                cancelar(x, pm, i, lista, etiqueta);
            }
        }
        _ => pm.rechazos += 1,
    }
}
