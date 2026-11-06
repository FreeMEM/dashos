//! Arranque de DashOS por UEFI (U1). Un programa de UEFI que el firmware (EDK2 en QEMU `virt`, y en su día el de un
//! portátil ARM o el U-Boot de un Mac) carga desde `EFI/BOOT/BOOTAA64.EFI`.
//!
//! Hoy: escribe por la consola de UEFI, busca el árbol de dispositivos en la tabla de configuración, pide el mapa de
//! memoria, sale de los servicios de arranque y, ya solo, escribe directamente en el puerto serie (PL011). Lo siguiente
//! (U2) es cargar el kernel y saltar a él con el árbol, como hace el firmware de la Pi.
//!
//! Escrito con la especificación de UEFI 2.10 (tablas del sistema y de servicios de arranque, protocolo de texto) y la
//! de devicetree (cabecera FDT).

#![no_std]
#![no_main]

use core::ffi::c_void;
use core::fmt::Write;

type Estado = usize;
const BIEN: Estado = 0;

#[repr(C)]
struct Cabecera {
    firma: u64,
    revision: u32,
    tamano: u32,
    crc32: u32,
    reservado: u32,
}

#[repr(C)]
struct SalidaTexto {
    reiniciar: extern "efiapi" fn(*mut SalidaTexto, bool) -> Estado,
    escribir: extern "efiapi" fn(*mut SalidaTexto, *const u16) -> Estado,
}

#[repr(C)]
struct Guid([u8; 16]);

#[repr(C)]
struct EntradaConfiguracion {
    guid: Guid,
    tabla: *const c_void,
}

#[repr(C)]
struct TablaSistema {
    cabecera: Cabecera,
    fabricante: *const u16,
    revision_firmware: u32,
    consola_entrada: *const c_void,
    entrada: *const c_void,
    consola_salida: *const c_void,
    salida: *mut SalidaTexto,
    error_estandar: *const c_void,
    error: *const c_void,
    servicios_ejecucion: *const c_void,
    servicios_arranque: *const ServiciosArranque,
    entradas_configuracion: usize,
    configuracion: *const EntradaConfiguracion,
}

type Reservado = usize;

#[repr(C)]
struct ServiciosArranque {
    cabecera: Cabecera,
    _tpl: [Reservado; 2],
    reservar_paginas: Reservado,
    liberar_paginas: Reservado,
    mapa_memoria: extern "efiapi" fn(*mut usize, *mut u8, *mut usize, *mut usize, *mut u32) -> Estado,
    _pool: [Reservado; 2],
    _eventos: [Reservado; 6],
    _protocolos: [Reservado; 3],
    _handle_protocol: Reservado,
    _reservado: Reservado,
    _registrar: Reservado,
    _localizar: [Reservado; 2],
    _instalar_configuracion: Reservado,
    _imagenes: [Reservado; 4],
    salir_servicios: extern "efiapi" fn(*const c_void, usize) -> Estado,
}

/// GUID de la tabla del árbol de dispositivos (EFI_DTB_TABLE_GUID), b1b621d5-f19c-41a5-830b-d9152c69aae0, en el orden
/// de bytes de memoria de un EFI_GUID (los tres primeros campos en little-endian).
const GUID_DTB: [u8; 16] = [0xd5, 0x21, 0xb6, 0xb1, 0x9c, 0xf1, 0xa5, 0x41, 0x83, 0x0b, 0xd9, 0x15, 0x2c, 0x69, 0xaa, 0xe0];

/// Escritura por la consola de UEFI, pasando el texto a UCS-2 por trozos (y "\n" a "\r\n").
struct Consola(*mut SalidaTexto);

impl Write for Consola {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let mut buf = [0u16; 128];
        let mut n = 0;
        for c in s.chars() {
            if n + 3 >= buf.len() {
                buf[n] = 0;
                unsafe { ((*self.0).escribir)(self.0, buf.as_ptr()) };
                n = 0;
            }
            if c == '\n' {
                buf[n] = '\r' as u16;
                n += 1;
            }
            buf[n] = if (c as u32) < 0x10000 { c as u16 } else { '?' as u16 };
            n += 1;
        }
        buf[n] = 0;
        unsafe { ((*self.0).escribir)(self.0, buf.as_ptr()) };
        Ok(())
    }
}

/// El PL011 de QEMU `virt`, para cuando ya no hay servicios de arranque. Provisional: en U2 sale del árbol.
const PL011_VIRT: usize = 0x0900_0000;

struct Serie;

impl Write for Serie {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                serie_byte(b'\r');
            }
            serie_byte(b);
        }
        Ok(())
    }
}

fn serie_byte(b: u8) {
    unsafe {
        while core::ptr::read_volatile((PL011_VIRT + 0x18) as *const u32) & (1 << 5) != 0 {}
        core::ptr::write_volatile(PL011_VIRT as *mut u32, b as u32);
    }
}

/// El árbol de dispositivos de la tabla de configuración: (dirección, tamaño según su cabecera).
fn buscar_dtb(st: &TablaSistema) -> Option<(usize, u32)> {
    for i in 0..st.entradas_configuracion {
        let e = unsafe { &*st.configuracion.add(i) };
        if e.guid.0 == GUID_DTB {
            let p = e.tabla as *const u32;
            // Cabecera FDT: magia 0xd00dfeed y tamaño total, en big-endian.
            let (magia, tamano) = unsafe { (u32::from_be(*p), u32::from_be(*p.add(1))) };
            if magia == 0xd00d_feed {
                return Some((p as usize, tamano));
            }
        }
    }
    None
}

static mut MAPA: [u8; 64 * 1024] = [0; 64 * 1024];

#[unsafe(export_name = "efi_main")]
extern "efiapi" fn principal(imagen: *const c_void, st: *const TablaSistema) -> Estado {
    let st = unsafe { &*st };
    let mut c = Consola(st.salida);
    let _ = writeln!(c, "DashOS: arranque por UEFI (revisión {:#x} del firmware)", st.cabecera.revision);
    let dtb = buscar_dtb(st);
    match dtb {
        Some((d, t)) => {
            let _ = writeln!(c, "DashOS: árbol de dispositivos en {:#x}, {} bytes", d, t);
        }
        None => {
            let _ = writeln!(c, "DashOS: el firmware no da árbol de dispositivos");
        }
    }
    // El mapa de memoria, y con su llave se sale de los servicios de arranque. Si el mapa cambió entre medias (el
    // firmware puede reservar al escribir), se vuelve a pedir.
    let bs = unsafe { &*st.servicios_arranque };
    let mapa = &raw mut MAPA;
    let mut salido = false;
    let mut regiones = 0;
    for _ in 0..4 {
        let (mut tamano, mut llave, mut tam_desc, mut version) = (unsafe { (*mapa).len() }, 0usize, 0usize, 0u32);
        if (bs.mapa_memoria)(&mut tamano, mapa as *mut u8, &mut llave, &mut tam_desc, &mut version) != BIEN {
            break;
        }
        regiones = if tam_desc > 0 { tamano / tam_desc } else { 0 };
        if (bs.salir_servicios)(imagen, llave) == BIEN {
            salido = true;
            break;
        }
    }
    if !salido {
        let _ = writeln!(c, "DashOS: no se pudo salir de los servicios de arranque");
        return 1;
    }
    // Desde aquí, sin firmware: solo el hardware.
    let _ = writeln!(Serie, "DashOS: fuera de los servicios de arranque; mapa de memoria con {} regiones", regiones);
    let _ = writeln!(Serie, "DashOS: U1 bien");
    loop {
        unsafe { core::arch::asm!("wfe") };
    }
}

#[panic_handler]
fn panico(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
