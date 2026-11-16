//! El controlador de virtio-net (S13, red con cable en QEMU), fuera del kernel. Escrito con la especificación virtio
//! 1.2: transporte PCI (4.1), colas partidas (2.7) y el aparato de red (5.1). Solo PCI moderno (en QEMU,
//! `disable-legacy=on`).
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 4); la 3, su espacio de configuración del PCIe; la 4, su
//! interrupción, si la hay; la siguiente, el canal "red" (el servicio). Sin interrupción, mira las colas cada 2 ms.
//!
//! Funciones: solo VERSION_1 y MAC. Sin sumas de comprobación por el aparato ni TSO: las hace la pila. Una cola de
//! recibir de 128 búferes de 2 KB y una de enviar de 64. Las tramas van y vienen del servicio por una memoria
//! compartida (`protocolo::red`, `T_*`): el controlador copia cada trama entre esa memoria y la DMA, así el servicio
//! nunca toca los descriptores. Lo que llega mal (corto, largo, con un descriptor que no es nuestro) se cuenta y se
//! tira; el aparato no se fía del servicio ni el servicio del aparato.

#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use dashos::println;
use dashos::protocolo::red as p;

dashos::programa!(principal);

const VA: usize = 0x0090_0000;
const VA_CFG: usize = 0x00A0_0000;
const VA_DMA: usize = 0x30_0000_0000;
const VA_TRAMAS: usize = 0x40_0000_0000;

// La memoria DMA: las dos colas y los búferes.
const M_RX: usize = 0x0000;
const M_TX: usize = 0x2000;
const M_RX_BUF: usize = 0x4000;
const BUFER: usize = 2048;
const N_RX: u16 = 128;
const N_TX: u16 = 64;
const M_TX_BUF: usize = M_RX_BUF + N_RX as usize * BUFER;
const TAM_DMA: usize = M_TX_BUF + N_TX as usize * BUFER;
/// La cabecera de virtio-net (5.1.6) con VERSION_1: 12 bytes delante de cada trama.
const CAB: usize = 12;

// Las ranuras compartidas con el servicio.
const RANURAS_RX: usize = 128;
const RANURAS_TX: usize = 64;
const TAM_TRAMAS: usize = 64 + (RANURAS_RX + RANURAS_TX) * p::RANURA;

const D_ESCRIBE: u16 = 2;

// Registros comunes (4.1.4.3).
const C_FUNC_ELEGIR: usize = 0x00;
const C_FUNC: usize = 0x04;
const C_PROPIAS_ELEGIR: usize = 0x08;
const C_PROPIAS: usize = 0x0C;
const C_ESTADO: usize = 0x14;
const C_COLA_ELEGIR: usize = 0x16;
const C_COLA_TAMANO: usize = 0x18;
const C_COLA_MSIX: usize = 0x1A;
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
/// VIRTIO_NET_F_MAC (5.1.3).
const F_MAC: u32 = 1 << 5;

const COLA_RX: u16 = 0;
const COLA_TX: u16 = 1;

/// Marcas de lo que llega por el canal propio.
const M_IRQ: u64 = 1;
const M_SERVICIO: u64 = 2;

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
/// Una palabra de la memoria de las tramas.
fn tramas(i: usize) -> u32 {
    unsafe { read_volatile((VA_TRAMAS + 4 * i) as *const u32) }
}
fn poner_tramas(i: usize, v: u32) {
    unsafe { write_volatile((VA_TRAMAS + 4 * i) as *mut u32, v) }
}
fn sumar(i: usize) {
    poner_tramas(i, tramas(i).wrapping_add(1));
}
fn barrera() {
    // Lo escrito en la memoria (DMA o compartida) se ve antes que lo que venga detrás (`dmb osh` en ARM; X5c: también
    // en x86).
    dashos::barrera::total();
}

/// Dónde están los bloques de registros dentro de la BAR (4.1.4).
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
                3 => isr = Some(desde),
                4 => aparato = Some(desde),
                _ => {}
            }
        }
        q = ((w >> 8) & 0xfc) as usize;
        vueltas += 1;
    }
    Some(Bloques { comun: comun?, isr: isr?, aviso: aviso?, multiplicador, aparato: aparato? })
}

/// Una cola partida en la memoria DMA: descriptores desde `m`, el anillo del conductor desde `m + disp` y el del
/// aparato desde `m + usados`.
struct Cola {
    m: usize,
    n: u16,
    disp_off: usize,
    usados_off: usize,
    aviso: usize,
    disp: u16,
    usados: u16,
}

impl Cola {
    fn descriptor(&self, i: u16, bus: u64, largo: u32, flags: u16) {
        let d = self.m + 16 * i as usize;
        poner64(d, bus);
        poner32(d + 8, largo);
        poner16(d + 12, flags);
        poner16(d + 14, 0);
    }

    fn largo(&self, i: u16, largo: u32) {
        poner32(self.m + 16 * i as usize + 8, largo);
    }

    /// Pone la cadena `cabeza` en el anillo del conductor, sin avisar todavía.
    fn poner(&mut self, cabeza: u16) {
        poner16(self.m + self.disp_off + 4 + 2 * (self.disp % self.n) as usize, cabeza);
        self.disp = self.disp.wrapping_add(1);
    }

    /// Publica lo puesto y avisa al aparato.
    fn avisar(&self, numero: u16) {
        barrera();
        poner16(self.m + self.disp_off + 2, self.disp);
        barrera();
        poner_reg16(self.aviso, numero);
    }

    /// La siguiente cadena que devolvió el aparato: (cabeza, largo escrito).
    fn devuelta(&mut self) -> Option<(u32, u32)> {
        if dma16(self.m + self.usados_off + 2) == self.usados {
            return None;
        }
        dashos::barrera::lecturas();
        let e = self.m + self.usados_off + 4 + 8 * (self.usados % self.n) as usize;
        self.usados = self.usados.wrapping_add(1);
        Some((dma32(e), dma32(e + 4)))
    }
}

/// Monta la cola `numero` en `m` con `n` entradas (2.7, 4.1.5.1.3).
fn montar(b: &Bloques, bus: u64, numero: u16, m: usize, n: u16, disp_off: usize, usados_off: usize) -> Option<Cola> {
    poner_reg16(b.comun + C_COLA_ELEGIR, numero);
    let max = reg16(b.comun + C_COLA_TAMANO);
    if max < n {
        return None;
    }
    poner_reg16(b.comun + C_COLA_TAMANO, n);
    let base = bus + m as u64;
    poner_reg64(b.comun + C_COLA_DESC, base);
    poner_reg64(b.comun + C_COLA_DISP, base + disp_off as u64);
    poner_reg64(b.comun + C_COLA_USADOS, base + usados_off as u64);
    let aviso = b.aviso + reg16(b.comun + C_COLA_AVISO) as usize * b.multiplicador as usize;
    // X5c: con MSI-X (x86), cada cola dice qué entrada de la tabla usa: la 0, la única que prepara el kernel (como en
    // `virtio-sonido`). Con INTx (ARM) no cuenta.
    poner_reg16(b.comun + C_COLA_MSIX, 0);
    poner_reg16(b.comun + C_COLA_ENCENDER, 1);
    Some(Cola { m, n, disp_off, usados_off, aviso, disp: 0, usados: 0 })
}

fn principal() -> u64 {
    // Con llave 5, la 4 es la interrupción y la 5 el canal "red"; si no, el canal es la 4.
    let existe = |k: u32| {
        let copia = dashos::llave_duplicar(k, dashos::LEER, 0);
        if copia > 0 {
            dashos::cerrar(copia as u32);
        }
        copia > 0
    };
    let (irq, servicio) = if existe(5) { (Some(4u32), 5u32) } else { (None, 4u32) };
    if dashos::registros_proyectar(2, VA) != 0 || dashos::registros_proyectar(3, VA_CFG) != 0 {
        println!("  [virtio-red] sin registros");
        return 1;
    }
    let Some(b) = bloques() else {
        println!("  [virtio-red] sin las capacidades de virtio en la BAR 4");
        return 2;
    };
    // Reiniciar, enterado, conductor (3.1.1).
    poner_reg8(b.comun + C_ESTADO, 0);
    let fin = dashos::ahora_us() + 100_000;
    while reg8(b.comun + C_ESTADO) != 0 && dashos::ahora_us() < fin {}
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO);
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO | S_CONDUCTOR);
    poner_reg32(b.comun + C_FUNC_ELEGIR, 1);
    let altas = reg32(b.comun + C_FUNC);
    if altas & 1 == 0 {
        println!("  [virtio-red] sin la versión 1");
        return 3;
    }
    poner_reg32(b.comun + C_FUNC_ELEGIR, 0);
    let con_mac = reg32(b.comun + C_FUNC) & F_MAC != 0;
    poner_reg32(b.comun + C_PROPIAS_ELEGIR, 0);
    poner_reg32(b.comun + C_PROPIAS, if con_mac { F_MAC } else { 0 });
    poner_reg32(b.comun + C_PROPIAS_ELEGIR, 1);
    // VERSION_1 y, si la ofrece, VIRTIO_F_ACCESS_PLATFORM (el bit 33): detrás de una IOMMU (X5b, X5c), con las
    // direcciones de bus que da `memoria_dma`. En ARM no la ofrece.
    poner_reg32(b.comun + C_PROPIAS, 1 | (altas & 2));
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO | S_CONDUCTOR | S_FUNC_OK);
    if reg8(b.comun + C_ESTADO) & S_FUNC_OK == 0 {
        println!("  [virtio-red] no acepta las funciones");
        return 4;
    }
    // El aparato ya está reiniciado: la DMA de una vida anterior se puede soltar.
    dashos::dma_liberar(1);
    let (m, bus) = dashos::memoria_dma(1, TAM_DMA);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_DMA, true) != 0 {
        println!("  [virtio-red] sin memoria DMA");
        return 5;
    }
    for i in (0..TAM_DMA).step_by(4) {
        poner32(i, 0);
    }
    let (Some(mut rx), Some(mut tx)) = (
        montar(&b, bus, COLA_RX, M_RX, N_RX, 0x800, 0x1000),
        montar(&b, bus, COLA_TX, M_TX, N_TX, 0x400, 0x800),
    ) else {
        println!("  [virtio-red] no monta las colas");
        return 6;
    };
    // La dirección MAC (5.1.4); sin ella, una local fija.
    let mut mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    if con_mac {
        for (i, x) in mac.iter_mut().enumerate() {
            *x = reg8(b.aparato + i);
        }
    }
    // Cada búfer de recibir, un descriptor que el aparato escribe; los de enviar se ponen al usarlos.
    for i in 0..N_RX {
        rx.descriptor(i, bus + (M_RX_BUF + i as usize * BUFER) as u64, BUFER as u32, D_ESCRIBE);
        rx.poner(i);
    }
    for i in 0..N_TX {
        tx.descriptor(i, bus + (M_TX_BUF + i as usize * BUFER) as u64, 0, 0);
    }
    poner_reg8(b.comun + C_ESTADO, S_ENTERADO | S_CONDUCTOR | S_FUNC_OK | S_LISTO);
    if reg8(b.comun + C_ESTADO) & S_FALLO != 0 {
        println!("  [virtio-red] el aparato falla al arrancar");
        return 7;
    }
    rx.avisar(COLA_RX);
    // La memoria de las tramas, para el servicio.
    let mem = dashos::memoria_crear(TAM_TRAMAS);
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_TRAMAS, true) != 0 {
        println!("  [virtio-red] sin memoria para las tramas");
        return 8;
    }
    poner_tramas(p::T_RANURAS_RX, RANURAS_RX as u32);
    poner_tramas(p::T_RANURAS_TX, RANURAS_TX as u32);
    // El canal propio: la interrupción (marca 1) y los avisos del servicio (marca 2).
    let (lee, escribe) = dashos::canal_crear();
    let para_servicio = dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, M_SERVICIO);
    let copia = dashos::llave_duplicar(mem, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let mut hola = [0u8; 12];
    hola[..4].copy_from_slice(&p::TARJETA.to_le_bytes());
    hola[4..10].copy_from_slice(&mac);
    if para_servicio <= 0 || copia <= 0 || dashos::canal_escribir(servicio, &hola, &[copia as u32, para_servicio as u32]) != 0 {
        println!("  [virtio-red] el servicio no recibe la tarjeta");
        return 9;
    }
    // La interrupción, por nivel (INTx): leer el estado (ISR) la baja, y después se vuelve a habilitar (4.1.4.5).
    let irq = irq.filter(|&i| {
        let marcada = dashos::llave_duplicar(escribe, dashos::ESCRIBIR, M_IRQ);
        marcada > 0 && dashos::interrupcion_asociar(i, marcada as u32) == 0
    });
    println!(
        "  [virtio-red] MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}; colas de {} y {}; avisos {}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], N_RX, N_TX,
        if irq.is_some() { "por su interrupción" } else { "con un temporizador" }
    );
    // Qué descriptores de enviar están en el aparato (bit i), y cuáles libres.
    let mut en_vuelo: u64 = 0;
    // Y qué búferes de recibir tiene el aparato: todos al empezar.
    let mut rx_dentro: u128 = u128::MAX >> (128 - N_RX as u32);
    let mut buf = [0u8; 16];
    let mut ll = [0u32; 8];
    let mut raros = 0u64;
    loop {
        let plazo = if irq.is_some() { 100_000 } else { 2_000 };
        let (n, k, marca) = dashos::canal_leer_plazo(lee, &mut buf, &mut ll, plazo);
        // Sin mensaje (plazo vencido), el número de llaves no vale nada.
        let k = if n >= 0 { k.min(ll.len()) } else { 0 };
        ll[..k].iter().for_each(|&l| {
            dashos::cerrar(l);
        });
        if n >= 0 && marca == M_IRQ {
            if let Some(i) = irq {
                reg8(b.isr);
                dashos::interrupcion_terminar(i);
            }
        }
        let mut movido = false;
        // Lo recibido: al anillo del servicio, si cabe y tiene buena medida.
        let mut repuestos = false;
        while let Some((id, largo)) = rx.devuelta() {
            // Un id que no es nuestro caería fuera de los búferes, y uno que ya volvió se daría dos veces: se cuenta y
            // no se toca.
            if id >= N_RX as u32 || rx_dentro & (1u128 << id) == 0 {
                raros += 1;
                continue;
            }
            rx_dentro &= !(1u128 << id);
            sumar(p::T_RECIBIDAS);
            let largo = largo as usize;
            if largo < CAB + p::MIN_TRAMA {
                sumar(p::T_CORTAS);
            } else if largo - CAB > p::MAX_TRAMA || largo > BUFER {
                sumar(p::T_LARGAS);
            } else {
                let (escritas, leidas) = (tramas(p::T_RX_ESCRITAS), tramas(p::T_RX_LEIDAS));
                if escritas.wrapping_sub(leidas) as usize >= RANURAS_RX {
                    sumar(p::T_SIN_SITIO);
                } else {
                    let r = VA_TRAMAS + 64 + (escritas as usize % RANURAS_RX) * p::RANURA;
                    let desde = VA_DMA + M_RX_BUF + id as usize * BUFER + CAB;
                    // De la DMA (sin caché) a la memoria compartida: bytes, sin interpretarlos.
                    unsafe { core::ptr::copy_nonoverlapping(desde as *const u8, (r + p::RANURA_DATOS) as *mut u8, largo - CAB) };
                    unsafe { write_volatile(r as *mut u32, (largo - CAB) as u32) };
                    barrera();
                    poner_tramas(p::T_RX_ESCRITAS, escritas.wrapping_add(1));
                    movido = true;
                }
            }
            rx.poner(id as u16);
            rx_dentro |= 1u128 << id;
            repuestos = true;
        }
        if repuestos {
            rx.avisar(COLA_RX);
        }
        // Lo enviado que vuelve: el descriptor queda libre.
        while let Some((id, _)) = tx.devuelta() {
            if id >= N_TX as u32 || en_vuelo & (1u64 << id) == 0 {
                raros += 1;
                continue;
            }
            en_vuelo &= !(1u64 << id);
        }
        // Lo que el servicio quiere enviar, mientras haya descriptores libres.
        let mut puestos = false;
        loop {
            let (escritas, leidas) = (tramas(p::T_TX_ESCRITAS), tramas(p::T_TX_LEIDAS));
            if escritas == leidas {
                break;
            }
            // El servicio no puede tener más ranuras llenas que las que hay: si lo dice, se tira todo.
            if escritas.wrapping_sub(leidas) as usize > RANURAS_TX {
                sumar(p::T_TX_MALAS);
                poner_tramas(p::T_TX_LEIDAS, escritas);
                movido = true;
                break;
            }
            let libre = (!en_vuelo).trailing_zeros();
            if libre >= N_TX as u32 {
                break;
            }
            barrera();
            let r = VA_TRAMAS + 64 + (RANURAS_RX + leidas as usize % RANURAS_TX) * p::RANURA;
            let largo = unsafe { read_volatile(r as *const u32) } as usize;
            if (p::MIN_TRAMA..=p::MAX_TRAMA).contains(&largo) {
                let dest = M_TX_BUF + libre as usize * BUFER;
                for i in (0..CAB).step_by(4) {
                    poner32(dest + i, 0);
                }
                unsafe { core::ptr::copy_nonoverlapping((r + p::RANURA_DATOS) as *const u8, (VA_DMA + dest + CAB) as *mut u8, largo) };
                tx.largo(libre as u16, (CAB + largo) as u32);
                tx.poner(libre as u16);
                en_vuelo |= 1u64 << libre;
                sumar(p::T_ENVIADAS);
                puestos = true;
            } else {
                sumar(p::T_TX_MALAS);
            }
            poner_tramas(p::T_TX_LEIDAS, leidas.wrapping_add(1));
            movido = true;
        }
        if puestos {
            tx.avisar(COLA_TX);
        }
        if movido {
            // Si la cola del servicio está llena, se pierde: el servicio mira las ranuras en cada vuelta.
            dashos::canal_escribir(servicio, &p::TRAMAS.to_le_bytes(), &[]);
        }
        if raros == 1 {
            println!("  [virtio-red] el aparato devuelve un descriptor que no es suyo; se ignora");
            raros += 1;
        }
    }
}
