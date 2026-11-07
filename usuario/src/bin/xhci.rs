//! El controlador USB por xHCI (U6): el de los portátiles y placas con UEFI, el de QEMU `virt` y, en la Pi 4, el
//! VL805 (por el PCIe). Escrito con la especificación de xHCI 1.2 de Intel (registros de capacidad, de operación y de
//! puertos, cap. 5).
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 0); la 3, el canal de entrada del compositor.
//!
//! Hoy (U6b): reinicia el controlador y dice qué puertos tienen algo conectado. Lo siguiente (U6c): los anillos de
//! órdenes y de eventos, dar dirección a los aparatos y leer el teclado y el ratón de arranque, como `usb.rs`.

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

fn velocidad(v: u32) -> &'static str {
    match v {
        1 => "completa (12 Mb/s)",
        2 => "baja (1,5 Mb/s)",
        3 => "alta (480 Mb/s)",
        4 => "super (5 Gb/s)",
        5 => "super+ (10 Gb/s)",
        _ => "?",
    }
}

fn principal() -> u64 {
    if dashos::registros_proyectar(2, VA) != 0 {
        println!("  [xhci] sin registros");
        return 1;
    }
    let cap = leer(CAPLENGTH);
    let op = (cap & 0xff) as usize;
    let version = (leer(HCIVERSION & !3) >> 16) & 0xffff;
    let params = leer(HCSPARAMS1);
    let puertos = (params >> 24) as usize;
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
    // Tras reiniciar, los puertos tardan un poco en ver lo conectado.
    esperar(100, || false);
    let mut conectados = 0;
    for p in 1..=puertos {
        let sc = leer(op + PORTSC + 0x10 * (p - 1));
        if sc & PORT_CCS != 0 {
            conectados += 1;
            println!("  [xhci] puerto {}: algo conectado, velocidad {}", p, velocidad((sc >> 10) & 0xf));
        }
    }
    println!("  [xhci] versión {:x}.{:02x}, {} puertos, {} con algo; reiniciado", version >> 8, version & 0xff, puertos, conectados);
    // Hasta U6c, espera sin gastar CPU.
    let (a, _b) = dashos::canal_crear();
    let mut buf = [0u8; 1];
    let mut ll = [0u32; 1];
    loop {
        dashos::canal_leer(a, &mut buf, &mut ll);
    }
}
