//! Arranque de DashOS por UEFI (U1). Un programa de UEFI que el firmware (EDK2 en QEMU `virt`, y en su día el de un
//! portátil ARM o el U-Boot de un Mac) carga desde `EFI/BOOT/BOOTAA64.EFI`.
//!
//! Hace lo que en la Pi hace su firmware (U2): pone la imagen del kernel en su dirección física, le prepara una copia
//! del árbol de dispositivos con las zonas que UEFI se queda (como `/memreserve/`), sale de los servicios de arranque,
//! apaga la MMU y salta al kernel con el árbol en `x0`.
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
    reservar_paginas: extern "efiapi" fn(u32, u32, usize, *mut u64) -> Estado,
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

/// La imagen del kernel y dónde va en la memoria física. En QEMU `virt` la RAM empieza en 0x40000000.
static NUCLEO: &[u8] = include_bytes!(env!("DASHOS_NUCLEO"));
const DIRECCION_NUCLEO: u64 = 0x4020_0000;
/// Lo que se reserva tras la imagen: pila de arranque y algo de margen (el kernel pone sus tablas y pilas detrás).
const MARGEN_NUCLEO: usize = 2 << 20;

// Reservar páginas (UEFI 2.10, 7.2): tipos de reserva y de memoria.
const EN_CUALQUIER_SITIO: u32 = 0;
const EN_ESA_DIRECCION: u32 = 2;
const DATOS_DEL_CARGADOR: u32 = 2;
const PAGINA: usize = 4096;

/// Tipos del mapa de memoria que quedan libres al salir de los servicios de arranque: código y datos del cargador y de
/// los servicios de arranque, y memoria convencional. Lo demás (servicios de ejecución, ACPI, reservado, inservible,
/// persistente...) lo sigue usando el firmware o no es RAM normal: va al árbol como zona reservada.
fn libre_al_salir(tipo: u32) -> bool {
    matches!(tipo, 1 | 2 | 3 | 4 | 7)
}

fn be32(p: *const u8) -> u32 {
    u32::from_be_bytes(unsafe { [*p, *p.add(1), *p.add(2), *p.add(3)] })
}

fn poner_be32(p: *mut u8, v: u32) {
    let b = v.to_be_bytes();
    for (i, x) in b.iter().enumerate() {
        unsafe { *p.add(i) = *x };
    }
}

/// Copia el árbol `origen` en `destino` con sus reservas y, detrás, las zonas reservadas de UEFI que da `reservas`.
/// Formato FDT (devicetree 0.4, cap. 5): cabecera de 40 bytes, mapa de reservas (pares de 64 bits big-endian acabados
/// en un par a cero), bloque de estructura y bloque de cadenas. Devuelve el tamaño nuevo, o `None` si no cabe.
fn copiar_arbol(origen: *const u8, destino: *mut u8, capacidad: usize, reservas: &[(u64, u64)]) -> Option<usize> {
    let (tam_estructura, off_estructura) = (be32(unsafe { origen.add(36) }) as usize, be32(unsafe { origen.add(8) }) as usize);
    let (tam_cadenas, off_cadenas) = (be32(unsafe { origen.add(32) }) as usize, be32(unsafe { origen.add(12) }) as usize);
    let off_reservas = be32(unsafe { origen.add(16) }) as usize;
    let mut viejas = 0;
    while unsafe { core::ptr::read_unaligned(origen.add(off_reservas + 16 * viejas) as *const [u64; 2]) } != [0, 0] {
        viejas += 1;
    }
    let n_reservas = viejas + reservas.len() + 1;
    let nuevo_reservas = 40usize.next_multiple_of(8);
    let nuevo_estructura = nuevo_reservas + 16 * n_reservas;
    let nuevo_cadenas = nuevo_estructura + tam_estructura;
    let total = nuevo_cadenas + tam_cadenas;
    if total > capacidad {
        return None;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(origen, destino, 40);
        core::ptr::copy_nonoverlapping(origen.add(off_reservas), destino.add(nuevo_reservas), 16 * viejas);
        for (k, &(dir, tam)) in reservas.iter().enumerate() {
            let e = destino.add(nuevo_reservas + 16 * (viejas + k));
            core::ptr::copy_nonoverlapping(dir.to_be_bytes().as_ptr(), e, 8);
            core::ptr::copy_nonoverlapping(tam.to_be_bytes().as_ptr(), e.add(8), 8);
        }
        core::ptr::write_bytes(destino.add(nuevo_reservas + 16 * (n_reservas - 1)), 0, 16);
        core::ptr::copy_nonoverlapping(origen.add(off_estructura), destino.add(nuevo_estructura), tam_estructura);
        core::ptr::copy_nonoverlapping(origen.add(off_cadenas), destino.add(nuevo_cadenas), tam_cadenas);
    }
    poner_be32(unsafe { destino.add(4) }, total as u32);
    poner_be32(unsafe { destino.add(8) }, nuevo_estructura as u32);
    poner_be32(unsafe { destino.add(12) }, nuevo_cadenas as u32);
    poner_be32(unsafe { destino.add(16) }, nuevo_reservas as u32);
    Some(total)
}

/// Lleva un tramo hasta el punto de coherencia: con la MMU apagada, el kernel lo leerá de la memoria, no de la caché.
fn limpiar(dir: usize, tam: usize) {
    let mut p = dir & !63;
    while p < dir + tam {
        unsafe { core::arch::asm!("dc civac, {}", in(reg) p) };
        p += 64;
    }
    unsafe { core::arch::asm!("dsb sy") };
}

/// Apaga la MMU y las cachés del nivel en que estemos (EL1 o EL2) y salta a `entrada` con `arbol` en x0. No vuelve.
fn saltar(entrada: u64, arbol: u64) -> ! {
    unsafe {
        core::arch::asm!(
            "msr daifset, #0xf",
            "mrs x3, CurrentEL",
            "cmp x3, #8",
            "b.eq 2f",
            "mrs x4, sctlr_el1",
            "bic x4, x4, #1",
            "bic x4, x4, #(1 << 2)",
            "bic x4, x4, #(1 << 12)",
            "msr sctlr_el1, x4",
            "b 3f",
            "2:",
            "mrs x4, sctlr_el2",
            "bic x4, x4, #1",
            "bic x4, x4, #(1 << 2)",
            "bic x4, x4, #(1 << 12)",
            "msr sctlr_el2, x4",
            "3:",
            "isb",
            "ic iallu",
            "tlbi vmalle1",
            "dsb sy",
            "isb",
            "br x1",
            in("x0") arbol,
            in("x1") entrada,
            options(noreturn)
        )
    }
}

static mut MAPA: [u8; 64 * 1024] = [0; 64 * 1024];

#[unsafe(export_name = "efi_main")]
extern "efiapi" fn principal(imagen: *const c_void, st: *const TablaSistema) -> Estado {
    let st = unsafe { &*st };
    let mut c = Consola(st.salida);
    let bs = unsafe { &*st.servicios_arranque };
    let _ = writeln!(c, "DashOS: arranque por UEFI (revisión {:#x} del firmware)", st.cabecera.revision);
    let Some((arbol, tam_arbol)) = buscar_dtb(st) else {
        let _ = writeln!(c, "DashOS: el firmware no da árbol de dispositivos");
        return 1;
    };
    let _ = writeln!(c, "DashOS: árbol de dispositivos en {:#x}, {} bytes", arbol, tam_arbol);
    // El kernel, en su dirección física. Si UEFI la tiene ocupada, no se puede seguir (el kernel no es reubicable).
    let mut dir = DIRECCION_NUCLEO;
    let paginas = (NUCLEO.len() + MARGEN_NUCLEO).div_ceil(PAGINA);
    if (bs.reservar_paginas)(EN_ESA_DIRECCION, DATOS_DEL_CARGADOR, paginas, &mut dir) != BIEN {
        let _ = writeln!(c, "DashOS: la memoria de {:#x} está ocupada; no se puede cargar el kernel", DIRECCION_NUCLEO);
        return 1;
    }
    unsafe { core::ptr::copy_nonoverlapping(NUCLEO.as_ptr(), dir as *mut u8, NUCLEO.len()) };
    let _ = writeln!(c, "DashOS: kernel de {} bytes en {:#x}", NUCLEO.len(), dir);
    // Sitio para la copia del árbol, con hueco para 128 zonas reservadas más.
    let capacidad = tam_arbol as usize + 16 * 128 + 64;
    let mut copia = 0u64;
    if (bs.reservar_paginas)(EN_CUALQUIER_SITIO, DATOS_DEL_CARGADOR, capacidad.div_ceil(PAGINA), &mut copia) != BIEN {
        let _ = writeln!(c, "DashOS: sin memoria para el árbol");
        return 1;
    }
    // El mapa de memoria, la copia del árbol con sus zonas reservadas y la salida de los servicios de arranque. Si el
    // mapa cambió entre medias, otra vuelta (aquí ya no se reserva nada).
    let mapa = &raw mut MAPA;
    let mut reservas = [(0u64, 0u64); 128];
    for _ in 0..4 {
        let (mut tamano, mut llave, mut tam_desc, mut version) = (unsafe { (*mapa).len() }, 0usize, 0usize, 0u32);
        if (bs.mapa_memoria)(&mut tamano, mapa as *mut u8, &mut llave, &mut tam_desc, &mut version) != BIEN || tam_desc == 0 {
            break;
        }
        // Descriptor (UEFI 2.10, 7.2): tipo (u32), relleno, inicio físico, inicio virtual, páginas, atributos.
        let mut n = 0;
        for k in 0..tamano / tam_desc {
            let d = unsafe { (mapa as *const u8).add(k * tam_desc) };
            let tipo = unsafe { core::ptr::read_unaligned(d as *const u32) };
            let inicio = unsafe { core::ptr::read_unaligned(d.add(8) as *const u64) };
            let pags = unsafe { core::ptr::read_unaligned(d.add(24) as *const u64) };
            if libre_al_salir(tipo) || pags == 0 {
                continue;
            }
            let tam = pags * PAGINA as u64;
            if n > 0 && reservas[n - 1].0 + reservas[n - 1].1 == inicio {
                reservas[n - 1].1 += tam; // seguida de la anterior: se juntan
            } else if n < reservas.len() {
                reservas[n] = (inicio, tam);
                n += 1;
            }
        }
        let Some(nuevo) = copiar_arbol(arbol as *const u8, copia as *mut u8, capacidad, &reservas[..n]) else { break };
        if (bs.salir_servicios)(imagen, llave) == BIEN {
            // Desde aquí, sin firmware: solo el hardware.
            let _ = writeln!(Serie, "DashOS: fuera de los servicios de arranque; árbol en {:#x}, {} bytes, {} zonas de UEFI reservadas", copia, nuevo, n);
            limpiar(dir as usize, NUCLEO.len());
            limpiar(copia as usize, nuevo);
            saltar(dir, copia);
        }
    }
    let _ = writeln!(c, "DashOS: no se pudo salir de los servicios de arranque");
    1
}

#[panic_handler]
fn panico(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
