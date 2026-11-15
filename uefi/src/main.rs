//! Arranque de DashOS por UEFI (U1). Un programa de UEFI que el firmware (EDK2 en QEMU `virt`, y en su día el de un
//! portátil ARM o el U-Boot de un Mac) carga desde `EFI/BOOT/BOOTAA64.EFI`.
//!
//! Hace lo que en la Pi hace su firmware (U2): pone la imagen del kernel en su dirección física, le prepara una copia
//! del árbol de dispositivos con las zonas que UEFI se queda (como `/memreserve/`), sale de los servicios de arranque,
//! apaga la MMU y salta al kernel con el árbol en `x0`.
//!
//! En x86-64 (S30, X1) el mismo programa sale como `EFI/BOOT/BOOTX64.EFI`; lo propio de x86 está en `x86_64.rs`.
//!
//! Escrito con la especificación de UEFI 2.10 (tablas del sistema y de servicios de arranque, protocolo de texto) y la
//! de devicetree (cabecera FDT).

#![no_std]
#![no_main]

use core::ffi::c_void;
use core::fmt::Write;

#[cfg(target_arch = "aarch64")]
mod acpi;
#[cfg(target_arch = "aarch64")]
mod fdt;
#[cfg(target_arch = "x86_64")]
mod x86_64;

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
    consultar_modo: extern "efiapi" fn(*const Gop, u32, *mut usize, *mut *const InfoGop) -> Estado,
    poner_modo: extern "efiapi" fn(*const Gop, u32) -> Estado,
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

/// fw_cfg de QEMU `virt` (docs/specs/fw_cfg.rst de QEMU): datos (+0), selector (+8, 16 bits) y DMA (+16, 64 bits), en
/// big-endian.
#[cfg(target_arch = "aarch64")]
const FW_CFG: usize = 0x0902_0000;

/// Solo para ver el escritorio a otro tamaño en QEMU (DASHOS_PANTALLA): la ramfb (fichero "etc/ramfb" de fw_cfg) a
/// `ancho` x `alto`, XRGB8888, sobre páginas reservadas (van al árbol como zona reservada, como las del firmware).
/// `None` si no hay ramfb o no se puede.
#[cfg(target_arch = "aarch64")]
fn ramfb(bs: &ServiciosArranque, c: &mut Consola, ancho: u32, alto: u32) -> Option<Pantalla> {
    let selector = |s: u16| unsafe { core::ptr::write_volatile((FW_CFG + 8) as *mut u16, s.to_be()) };
    let byte = || unsafe { core::ptr::read_volatile(FW_CFG as *const u8) };
    // El directorio de ficheros (selector 0x19): cuántos (32 bits) y entradas de 64 bytes (tamaño 32, selector 16,
    // reservado 16, nombre 56).
    selector(0x19);
    let mut n = [0u8; 4];
    n.iter_mut().for_each(|b| *b = byte());
    let mut sel = None;
    for _ in 0..u32::from_be_bytes(n).min(512) {
        let mut e = [0u8; 64];
        e.iter_mut().for_each(|b| *b = byte());
        let fin = e[8..].iter().position(|&x| x == 0).unwrap_or(56);
        if &e[8..8 + fin] == b"etc/ramfb" {
            sel = Some(u16::from_be_bytes([e[4], e[5]]));
        }
    }
    let sel = sel?;
    let tamano = ancho as u64 * alto as u64 * 4;
    let mut base = DEBAJO_DE_4GB;
    // Por debajo de 4 GB, memoria 0 (reservada).
    if (bs.reservar_paginas)(POR_DEBAJO_DE, 0, tamano.div_ceil(4096) as usize, &mut base) != BIEN {
        return None;
    }
    unsafe { core::ptr::write_bytes(base as *mut u8, 0, tamano as usize) };
    // RAMFBCfg: dirección (64), formato (XR24), flags, ancho, alto y bytes por línea (32 cada uno).
    #[repr(C, align(8))]
    struct Cfg([u8; 28]);
    let mut cfg = Cfg([0; 28]);
    cfg.0[0..8].copy_from_slice(&base.to_be_bytes());
    cfg.0[8..12].copy_from_slice(&0x3432_5258u32.to_be_bytes());
    cfg.0[16..20].copy_from_slice(&ancho.to_be_bytes());
    cfg.0[20..24].copy_from_slice(&alto.to_be_bytes());
    cfg.0[24..28].copy_from_slice(&(ancho * 4).to_be_bytes());
    // FWCfgDmaAccess: control (selector << 16, elegir 0x08 y escribir 0x10), largo y dirección. Se manda al escribir
    // la mitad baja de su dirección.
    #[repr(C, align(16))]
    struct Dma {
        control: u32,
        largo: u32,
        dir: u64,
    }
    let dma = Dma { control: ((sel as u32) << 16 | 0x18).to_be(), largo: 28u32.to_be(), dir: (&cfg as *const Cfg as u64).to_be() };
    let p = &dma as *const Dma as u64;
    unsafe {
        core::ptr::write_volatile((FW_CFG + 16) as *mut u32, ((p >> 32) as u32).to_be());
        core::ptr::write_volatile((FW_CFG + 20) as *mut u32, (p as u32).to_be());
    }
    for _ in 0..10_000_000 {
        let control = u32::from_be(unsafe { core::ptr::read_volatile(&dma.control) });
        if control & !1 == 0 {
            if control & 1 != 0 {
                break;
            }
            let _ = writeln!(c, "DashOS: ramfb a {}x{} en {:#x}", ancho, alto, base);
            return Some(Pantalla { base, tamano, ancho, alto, por_linea: ancho, formato: 1 });
        }
    }
    let _ = writeln!(c, "DashOS: la ramfb no acepta {}x{}", ancho, alto);
    None
}

fn buscar_pantalla(bs: &ServiciosArranque, c: &mut Consola) -> Option<Pantalla> {
    let mut gop: *const Gop = core::ptr::null();
    if (bs.localizar_protocolo)(&GUID_GOP, core::ptr::null(), &mut gop) != BIEN || gop.is_null() {
        return None;
    }
    // Con DASHOS_PANTALLA=ANCHOxALTO al compilar, se pide ese modo si el firmware lo tiene (para ver el escritorio a
    // otro tamaño; las pruebas cuentan con el de serie, 800 × 600 en QEMU).
    if let Some((ancho, alto)) = option_env!("DASHOS_PANTALLA").and_then(|t| t.split_once('x')).and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))) {
        let max = unsafe { (*(*gop).modo).max_modo };
        let _ = writeln!(c, "DashOS: el firmware da {} modos de pantalla", max);
        for m in 0..max {
            let mut tam = 0usize;
            let mut info: *const InfoGop = core::ptr::null();
            if (unsafe { (*gop).consultar_modo })(gop, m, &mut tam, &mut info) == BIEN && !info.is_null() {
                let i = unsafe { &*info };
                if i.ancho == ancho && i.alto == alto {
                    let e = (unsafe { (*gop).poner_modo })(gop, m);
                    let _ = writeln!(c, "DashOS: modo {} de {} ({}x{}) pedido: {}", m, max, ancho, alto, if e == BIEN { "puesto" } else { "rechazado" });
                    break;
                }
            }
        }
        // Sin ese modo en el firmware (el de QEMU, por ramfb, solo da 640x480, 800x600 y 1024x768): en `virt`, se
        // programa la ramfb a ese tamaño por fw_cfg, con memoria propia.
        #[cfg(target_arch = "aarch64")]
        let actual = unsafe { &*(*(*gop).modo).info };
        #[cfg(target_arch = "aarch64")]
        if (actual.ancho, actual.alto) != (ancho, alto) {
            if let Some(p) = ramfb(bs, c, ancho, alto) {
                return Some(p);
            }
        }
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
#[cfg(target_arch = "aarch64")]
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
#[cfg(target_arch = "aarch64")]
const PL011_VIRT: usize = 0x0900_0000;

#[cfg(target_arch = "aarch64")]
struct Serie;

#[cfg(target_arch = "aarch64")]
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

#[cfg(target_arch = "aarch64")]
fn serie_byte(b: u8) {
    unsafe {
        while core::ptr::read_volatile((PL011_VIRT + 0x18) as *const u32) & (1 << 5) != 0 {}
        core::ptr::write_volatile(PL011_VIRT as *mut u32, b as u32);
    }
}

/// El árbol de dispositivos de la tabla de configuración: (dirección, tamaño según su cabecera).
#[cfg(target_arch = "aarch64")]
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
#[cfg(target_arch = "aarch64")]
static NUCLEO: &[u8] = include_bytes!(env!("DASHOS_NUCLEO"));
#[cfg(target_arch = "aarch64")]
const DIRECCION_NUCLEO: u64 = 0x4020_0000;
/// Lo que se reserva tras la imagen: pila de arranque y algo de margen (el kernel pone sus tablas y pilas detrás).
#[cfg(target_arch = "aarch64")]
const MARGEN_NUCLEO: usize = 2 << 20;

// Reservar páginas (UEFI 2.10, 7.2): tipos de reserva y de memoria.
/// Por debajo de la dirección que se pasa (caja-linux-grandes: con highmem, QEMU `virt` tiene RAM por encima de los 4
/// GB y UEFI reparte desde arriba; el árbol y la pantalla se quedan por debajo).
#[cfg(target_arch = "aarch64")]
const POR_DEBAJO_DE: u32 = 1;
const EN_ESA_DIRECCION: u32 = 2;
#[cfg(target_arch = "aarch64")]
const DEBAJO_DE_4GB: u64 = 0xFFFF_FFFF;
#[cfg(target_arch = "aarch64")]
const DATOS_DEL_CARGADOR: u32 = 2;
const PAGINA: usize = 4096;

/// Tipos del mapa de memoria que quedan libres al salir de los servicios de arranque: código y datos del cargador y de
/// los servicios de arranque, y memoria convencional. Lo demás (servicios de ejecución, ACPI, reservado, inservible,
/// persistente...) lo sigue usando el firmware o no es RAM normal: va al árbol como zona reservada.
#[cfg(target_arch = "aarch64")]
fn libre_al_salir(tipo: u32) -> bool {
    matches!(tipo, 1 | 2 | 3 | 4 | 7)
}

#[cfg(target_arch = "aarch64")]
fn be32(p: *const u8) -> u32 {
    u32::from_be_bytes(unsafe { [*p, *p.add(1), *p.add(2), *p.add(3)] })
}

#[cfg(target_arch = "aarch64")]
fn poner_be32(p: *mut u8, v: u32) {
    let b = v.to_be_bytes();
    for (i, x) in b.iter().enumerate() {
        unsafe { *p.add(i) = *x };
    }
}

/// Un nodo `simple-framebuffer` (devicetree, `bindings/display/simple-framebuffer.yaml`) para la pantalla del firmware:
/// sus tokens del bloque de estructura y las cadenas que usan. Supone `#address-cells` y `#size-cells` de 2 en la raíz
/// (los de QEMU `virt` y de las máquinas de 64 bits). Las cadenas van detrás de las del árbol, desde `base_cadenas`.
#[cfg(target_arch = "aarch64")]
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
#[cfg(target_arch = "aarch64")]
struct Trozo<'a>(&'a mut [u8], usize);

#[cfg(target_arch = "aarch64")]
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
#[cfg(target_arch = "aarch64")]
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
#[cfg(target_arch = "aarch64")]
fn limpiar(dir: usize, tam: usize) {
    let mut p = dir & !63;
    while p < dir + tam {
        unsafe { core::arch::asm!("dc civac, {}", in(reg) p) };
        p += 64;
    }
    unsafe { core::arch::asm!("dsb sy") };
}

/// Apaga la MMU y las cachés del nivel en que estemos (EL1 o EL2) y salta a `entrada` con `arbol` en x0. No vuelve.
#[cfg(target_arch = "aarch64")]
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

/// La tabla de configuración con ese GUID, si la hay.
fn tabla_config(st: &TablaSistema, guid: &[u8; 16]) -> Option<usize> {
    (0..st.entradas_configuracion)
        .map(|i| unsafe { &*st.configuracion.add(i) })
        .find(|e| &e.guid.0 == guid)
        .map(|e| e.tabla as usize)
}

/// Los tramos de RAM del mapa de memoria de UEFI, juntando los seguidos (U5: para el árbol hecho desde ACPI). Es RAM
/// todo salvo lo reservado, lo inservible y la entrada y salida proyectadas (tipos 0, 8, 11, 12 y 13).
#[cfg(target_arch = "aarch64")]
fn tramos_de_ram(bs: &ServiciosArranque, ram: &mut [(u64, u64); 32]) -> usize {
    let mapa = &raw mut MAPA;
    let (mut tamano, mut llave, mut tam_desc, mut version) = (unsafe { (*mapa).len() }, 0usize, 0usize, 0u32);
    if (bs.mapa_memoria)(&mut tamano, mapa as *mut u8, &mut llave, &mut tam_desc, &mut version) != BIEN || tam_desc == 0 {
        return 0;
    }
    let mut n = 0;
    for k in 0..tamano / tam_desc {
        let d = unsafe { (mapa as *const u8).add(k * tam_desc) };
        let tipo = unsafe { core::ptr::read_unaligned(d as *const u32) };
        let inicio = unsafe { core::ptr::read_unaligned(d.add(8) as *const u64) };
        let tam = unsafe { core::ptr::read_unaligned(d.add(24) as *const u64) } * PAGINA as u64;
        if matches!(tipo, 0 | 8 | 11 | 12 | 13) || tam == 0 {
            continue;
        }
        if n > 0 && ram[n - 1].0 + ram[n - 1].1 == inicio {
            ram[n - 1].1 += tam;
        } else if n < ram.len() {
            ram[n] = (inicio, tam);
            n += 1;
        }
    }
    // El mapa no viene ordenado por fuerza: se ordena (son pocos).
    ram[..n].sort_unstable_by_key(|t| t.0);
    n
}

/// El árbol hecho desde ACPI (U5), aquí hasta que se copie.
#[cfg(target_arch = "aarch64")]
static mut ARBOL_ACPI: [u8; 16384] = [0; 16384];

static mut MAPA: [u8; 64 * 1024] = [0; 64 * 1024];

#[cfg(target_arch = "aarch64")]
#[unsafe(export_name = "efi_main")]
extern "efiapi" fn principal(imagen: *const c_void, st: *const TablaSistema) -> Estado {
    let st = unsafe { &*st };
    let mut c = Consola(st.salida);
    let bs = unsafe { &*st.servicios_arranque };
    let _ = writeln!(c, "DashOS: arranque por UEFI (revisión {:#x} del firmware)", st.cabecera.revision);
    // El árbol del firmware; si no lo hay, uno hecho desde ACPI (U5: los portátiles dan solo ACPI).
    let (arbol, tam_arbol) = match buscar_dtb(st) {
        Some(a) => a,
        None => {
            let Some(rsdp) = tabla_config(st, &acpi::GUID_ACPI2) else {
                let _ = writeln!(c, "DashOS: el firmware no da árbol de dispositivos ni ACPI");
                return 1;
            };
            let Some(m) = acpi::leer(rsdp) else {
                let _ = writeln!(c, "DashOS: ACPI sin lo que hace falta (MADT con el GIC)");
                return 1;
            };
            let mut ram = [(0u64, 0u64); 32];
            let n = tramos_de_ram(bs, &mut ram);
            let buf = unsafe { &mut *(&raw mut ARBOL_ACPI) };
            let Some(t) = acpi::arbol(&m, &ram[..n], buf) else {
                let _ = writeln!(c, "DashOS: el árbol hecho desde ACPI no cabe");
                return 1;
            };
            let _ = writeln!(c, "DashOS: sin árbol; hecho desde ACPI: GIC{} en {:#x}, serie {:?}, PSCI {:?}, {} tramos de RAM", if m.gic_v3 { 3 } else { 2 }, m.distribuidor, m.serie, m.psci, n);
            for &(base, salida, cuantos) in &m.msi_map[..m.msi_n] {
                let _ = writeln!(c, "DashOS: IORT: RID {:#x} a {:#x} al ITS como {:#x}", base, base as u64 + cuantos as u64 - 1, salida);
            }
            (buf.as_ptr() as usize, t as u32)
        }
    };
    let _ = writeln!(c, "DashOS: árbol de dispositivos en {:#x}, {} bytes", arbol, tam_arbol);
    let pantalla = buscar_pantalla(bs, &mut c);
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
    let mut copia = DEBAJO_DE_4GB;
    if (bs.reservar_paginas)(POR_DEBAJO_DE, DATOS_DEL_CARGADOR, capacidad.div_ceil(PAGINA), &mut copia) != BIEN {
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
        // La pantalla del firmware, también reservada (multimonitor): con virtio-gpu, la de UEFI sale de memoria de los
        // servicios de arranque, que queda libre al salir; sin reservarla, el kernel podría dar a otro esa memoria
        // mientras el compositor aún pinta en ella (hasta que llega el controlador de virtio-gpu).
        if let Some(p) = pantalla.as_ref().filter(|p| p.tamano > 0)
            && n < reservas.len() {
                let desde = p.base & !(PAGINA as u64 - 1);
                reservas[n] = (desde, (p.base + p.tamano).div_ceil(PAGINA as u64) * PAGINA as u64 - desde);
                n += 1;
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
