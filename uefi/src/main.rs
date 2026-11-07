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
    _contador_reloj_vigilante: [Reservado; 3],
    _controladores: [Reservado; 2],
    _abrir_protocolos: [Reservado; 3],
    _protocolos_por_handle: Reservado,
    _localizar_handles: Reservado,
    localizar_protocolo: extern "efiapi" fn(*const [u8; 16], *const c_void, *mut *const Gop) -> Estado,
}

/// EFI_GRAPHICS_OUTPUT_PROTOCOL (UEFI 2.10, 12.9): solo lo que hace falta para saber dónde está la pantalla.
#[repr(C)]
struct Gop {
    _consultar_modo: Reservado,
    _poner_modo: Reservado,
    _blt: Reservado,
    modo: *const ModoGop,
}

#[repr(C)]
struct ModoGop {
    max_modo: u32,
    modo: u32,
    info: *const InfoGop,
    tam_info: usize,
    base: u64,
    tamano: usize,
}

#[repr(C)]
struct InfoGop {
    version: u32,
    ancho: u32,
    alto: u32,
    /// 0 RGB de 8 bits y uno libre, 1 BGR, 2 por máscaras, 3 solo Blt (sin búfer).
    formato: u32,
    _mascaras: [u32; 4],
    pixeles_por_linea: u32,
}

/// EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID, 9042a9de-23dc-4a38-96fb-7aded080516a, en orden de memoria.
const GUID_GOP: [u8; 16] = [0xde, 0xa9, 0x42, 0x90, 0xdc, 0x23, 0x38, 0x4a, 0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a];

/// La pantalla que deja el firmware: (base física, tamaño en bytes, ancho, alto, píxeles por línea, formato).
#[derive(Clone, Copy)]
struct Pantalla {
    base: u64,
    tamano: u64,
    ancho: u32,
    alto: u32,
    por_linea: u32,
    formato: u32,
}

fn buscar_pantalla(bs: &ServiciosArranque) -> Option<Pantalla> {
    let mut gop: *const Gop = core::ptr::null();
    if (bs.localizar_protocolo)(&GUID_GOP, core::ptr::null(), &mut gop) != BIEN || gop.is_null() {
        return None;
    }
    let modo = unsafe { &*(*gop).modo };
    let info = unsafe { &*modo.info };
    // Solo un búfer lineal de 32 bits por píxel (formatos 0 y 1): con "solo Blt" no hay memoria que pintar.
    if info.formato > 1 || modo.base == 0 {
        return None;
    }
    Some(Pantalla { base: modo.base, tamano: modo.tamano as u64, ancho: info.ancho, alto: info.alto, por_linea: info.pixeles_por_linea, formato: info.formato })
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

/// Un nodo `simple-framebuffer` (devicetree, `bindings/display/simple-framebuffer.yaml`) para la pantalla del firmware:
/// sus tokens del bloque de estructura y las cadenas que usan. Supone `#address-cells` y `#size-cells` de 2 en la raíz
/// (los de QEMU `virt` y de las máquinas de 64 bits). Las cadenas van detrás de las del árbol, desde `base_cadenas`.
fn nodo_pantalla(p: &Pantalla, base_cadenas: u32, estructura: &mut [u8; 256], cadenas: &mut [u8; 64]) -> (usize, usize) {
    const NOMBRES: &[u8] = b"compatible\0reg\0width\0height\0stride\0format\0status\0";
    cadenas[..NOMBRES.len()].copy_from_slice(NOMBRES);
    let desp = |nombre: &[u8]| -> u32 {
        let i = NOMBRES.windows(nombre.len()).position(|w| w == nombre).unwrap();
        base_cadenas + i as u32
    };
    let mut n = 0;
    let mut poner = |b: &[u8]| {
        estructura[n..n + b.len()].copy_from_slice(b);
        n += b.len().next_multiple_of(4);
    };
    poner(&1u32.to_be_bytes()); // FDT_BEGIN_NODE
    let mut nombre = [0u8; 28];
    let _ = write!(Trozo(&mut nombre[..], 0), "framebuffer@{:x}", p.base);
    let largo = nombre.iter().position(|&b| b == 0).unwrap_or(27) + 1;
    poner(&nombre[..largo]);
    let mut prop = |nombre: &[u8], valor: &[u8]| {
        poner(&3u32.to_be_bytes()); // FDT_PROP
        poner(&(valor.len() as u32).to_be_bytes());
        poner(&desp(nombre).to_be_bytes());
        poner(valor);
    };
    prop(b"compatible\0", b"simple-framebuffer\0");
    let mut reg = [0u8; 16];
    reg[..8].copy_from_slice(&p.base.to_be_bytes());
    reg[8..].copy_from_slice(&p.tamano.to_be_bytes());
    prop(b"reg\0", &reg);
    prop(b"width\0", &p.ancho.to_be_bytes());
    prop(b"height\0", &p.alto.to_be_bytes());
    prop(b"stride\0", &(p.por_linea * 4).to_be_bytes());
    // GOP 1 es B, G, R y uno libre en memoria: a8r8g8b8 en little-endian; GOP 0, R, G, B: a8b8g8r8.
    prop(b"format\0", if p.formato == 1 { b"a8r8g8b8\0" } else { b"a8b8g8r8\0" });
    prop(b"status\0", b"okay\0");
    poner(&2u32.to_be_bytes()); // FDT_END_NODE
    (n, NOMBRES.len())
}

/// Escribe texto con formato en un trozo de bytes, cortando lo que no quepa.
struct Trozo<'a>(&'a mut [u8], usize);

impl Write for Trozo<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            if self.1 + 1 < self.0.len() {
                self.0[self.1] = b;
                self.1 += 1;
            }
        }
        Ok(())
    }
}

/// Copia el árbol `origen` en `destino` con sus reservas y, detrás, las zonas reservadas de UEFI que da `reservas`.
/// Formato FDT (devicetree 0.4, cap. 5): cabecera de 40 bytes, mapa de reservas (pares de 64 bits big-endian acabados
/// en un par a cero), bloque de estructura y bloque de cadenas. Devuelve el tamaño nuevo, o `None` si no cabe.
/// Con `pantalla`, añade al final de la raíz su nodo `simple-framebuffer` (U4).
fn copiar_arbol(origen: *const u8, destino: *mut u8, capacidad: usize, reservas: &[(u64, u64)], pantalla: Option<&Pantalla>) -> Option<usize> {
    let (tam_estructura, off_estructura) = (be32(unsafe { origen.add(36) }) as usize, be32(unsafe { origen.add(8) }) as usize);
    let (tam_cadenas, off_cadenas) = (be32(unsafe { origen.add(32) }) as usize, be32(unsafe { origen.add(12) }) as usize);
    let off_reservas = be32(unsafe { origen.add(16) }) as usize;
    let mut viejas = 0;
    while unsafe { core::ptr::read_unaligned(origen.add(off_reservas + 16 * viejas) as *const [u64; 2]) } != [0, 0] {
        viejas += 1;
    }
    let n_reservas = viejas + reservas.len() + 1;
    let (mut nodo, mut nuevas_cadenas) = ([0u8; 256], [0u8; 64]);
    let (tam_nodo, tam_nuevas) = match pantalla {
        Some(p) => nodo_pantalla(p, tam_cadenas as u32, &mut nodo, &mut nuevas_cadenas),
        None => (0, 0),
    };
    let nuevo_reservas = 40usize.next_multiple_of(8);
    let nuevo_estructura = nuevo_reservas + 16 * n_reservas;
    let tam_estructura_nueva = tam_estructura + tam_nodo;
    let nuevo_cadenas = nuevo_estructura + tam_estructura_nueva;
    let total = nuevo_cadenas + tam_cadenas + tam_nuevas;
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
        // La estructura acaba con el fin de la raíz (FDT_END_NODE) y FDT_END: el nodo nuevo va justo antes.
        let antes = tam_estructura - 8;
        core::ptr::copy_nonoverlapping(origen.add(off_estructura), destino.add(nuevo_estructura), antes);
        core::ptr::copy_nonoverlapping(nodo.as_ptr(), destino.add(nuevo_estructura + antes), tam_nodo);
        core::ptr::copy_nonoverlapping(origen.add(off_estructura + antes), destino.add(nuevo_estructura + antes + tam_nodo), 8);
        core::ptr::copy_nonoverlapping(origen.add(off_cadenas), destino.add(nuevo_cadenas), tam_cadenas);
        core::ptr::copy_nonoverlapping(nuevas_cadenas.as_ptr(), destino.add(nuevo_cadenas + tam_cadenas), tam_nuevas);
    }
    poner_be32(unsafe { destino.add(32) }, (tam_cadenas + tam_nuevas) as u32);
    poner_be32(unsafe { destino.add(36) }, tam_estructura_nueva as u32);
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
    let pantalla = buscar_pantalla(bs);
    match pantalla {
        Some(p) => {
            let _ = writeln!(c, "DashOS: pantalla del firmware de {}x{} ({} por línea, formato {}) en {:#x}, {} KB", p.ancho, p.alto, p.por_linea, p.formato, p.base, p.tamano / 1024);
        }
        None => {
            let _ = writeln!(c, "DashOS: el firmware no da pantalla");
        }
    }
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
    let capacidad = tam_arbol as usize + 16 * 128 + 64 + 256 + 64;
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
        let Some(nuevo) = copiar_arbol(arbol as *const u8, copia as *mut u8, capacidad, &reservas[..n], pantalla.as_ref()) else { break };
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
