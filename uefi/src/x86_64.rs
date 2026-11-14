//! El cargador en x86-64 (S21, X1): el mismo programa de UEFI, como `EFI/BOOT/BOOTX64.EFI`.
//!
//! Lleva dentro el kernel compilado para `x86_64-unknown-none`, como ELF. Pone sus segmentos en su dirección física
//! (la de carga, `p_paddr`: desde X2 la virtual está en la mitad alta),
//! junta lo que el kernel necesita (el mapa de memoria de UEFI, el puntero RSDP de ACPI y la pantalla del GOP) en una
//! `InfoArranque`, sale de los servicios de arranque y salta a la entrada del ELF, en su dirección física, con la
//! dirección de la info en `rdi`,
//! las interrupciones cerradas y las tablas de páginas del firmware, que proyectan la memoria física 1:1.
//!
//! Escrito con la especificación de UEFI 2.10 (7.2, servicios de memoria; 4.6, tabla de configuración), la de ACPI
//! 6.5 (5.2.5, RSDP) y la de ELF-64 (cabecera y cabeceras de programa).

use crate::*;

/// El kernel, como ELF de x86-64 (lo da el Makefile del kernel en `DASHOS_NUCLEO`).
static NUCLEO: &[u8] = include_bytes!(env!("DASHOS_NUCLEO"));

/// Tipo de memoria del kernel: código del cargador. OVMF marca los datos del cargador como no ejecutables.
const CODIGO_DEL_CARGADOR: u32 = 1;

/// EFI_ACPI_20_TABLE_GUID, 8868e871-e4f1-11d3-bc22-0080c73c8881, y el de ACPI 1.0 (ACPI_TABLE_GUID),
/// eb9d2d30-2d88-11d3-9a16-0090273fc14d, en orden de memoria.
const GUID_ACPI2: [u8; 16] = [0x71, 0xe8, 0x68, 0x88, 0xf1, 0xe4, 0xd3, 0x11, 0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81];
const GUID_ACPI1: [u8; 16] = [0x30, 0x2d, 0x9d, 0xeb, 0x88, 0x2d, 0xd3, 0x11, 0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d];

/// Lo que el cargador deja al kernel. La misma estructura está en `kernel/src/base/x86_64/arranque.rs`: si cambia
/// aquí, cambia allí (y `VERSION`).
#[repr(C)]
struct InfoArranque {
    /// `MARCA`: que el kernel sepa que lo que tiene en `rdi` es esto.
    marca: u64,
    version: u32,
    _relleno: u32,
    /// Dirección física de la RSDP de ACPI, o 0.
    rsdp: u64,
    /// El mapa de memoria de UEFI tal cual: dirección, tamaño en bytes, tamaño de cada descriptor y su versión.
    mapa: u64,
    mapa_tamano: u64,
    mapa_descriptor: u64,
    mapa_version: u32,
    _relleno2: u32,
    /// La pantalla del GOP: base física y tamaño en bytes (0 si no hay), ancho, alto, píxeles por línea y formato
    /// (0 RGB, 1 BGR, con un byte libre).
    pantalla_base: u64,
    pantalla_tamano: u64,
    pantalla_ancho: u32,
    pantalla_alto: u32,
    pantalla_por_linea: u32,
    pantalla_formato: u32,
}

const MARCA: u64 = u64::from_le_bytes(*b"DashOS64");
const VERSION: u32 = 1;

static mut INFO: InfoArranque = InfoArranque {
    marca: 0,
    version: 0,
    _relleno: 0,
    rsdp: 0,
    mapa: 0,
    mapa_tamano: 0,
    mapa_descriptor: 0,
    mapa_version: 0,
    _relleno2: 0,
    pantalla_base: 0,
    pantalla_tamano: 0,
    pantalla_ancho: 0,
    pantalla_alto: 0,
    pantalla_por_linea: 0,
    pantalla_formato: 0,
};

fn u16_en(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn u32_en(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn u64_en(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(i..i + 8)?.try_into().ok()?))
}

/// Un segmento que se carga (PT_LOAD): desde dónde en el fichero, a qué dirección física y en qué virtual, cuánto hay
/// en el fichero y cuánto ocupa en memoria (lo de más, a cero).
#[derive(Clone, Copy, Default)]
struct Segmento {
    desde: usize,
    dir: u64,
    virt: u64,
    en_fichero: u64,
    en_memoria: u64,
}

/// El ELF del kernel: entrada (física), sus segmentos y el tramo físico que cubren. Solo un ejecutable de 64 bits,
/// little-endian, de x86-64. La entrada del ELF es virtual: se pasa a física con el segmento que la contiene, y el
/// kernel empieza con la proyección 1:1 del firmware (arranque.s del kernel).
struct Elf {
    entrada: u64,
    segmentos: [Segmento; 8],
    n: usize,
    inicio: u64,
    fin: u64,
}

fn leer_elf(b: &[u8]) -> Result<Elf, &'static str> {
    if b.get(..4) != Some(b"\x7fELF") || b.get(4) != Some(&2) || b.get(5) != Some(&1) {
        return Err("no es un ELF de 64 bits little-endian");
    }
    if u16_en(b, 16) != Some(2) || u16_en(b, 18) != Some(0x3e) {
        return Err("no es un ejecutable de x86-64");
    }
    let entrada = u64_en(b, 24).ok_or("cabecera corta")?;
    let desde = u64_en(b, 32).ok_or("cabecera corta")? as usize;
    let tam = u16_en(b, 54).ok_or("cabecera corta")? as usize;
    let cuantas = u16_en(b, 56).ok_or("cabecera corta")? as usize;
    let mut e = Elf { entrada, segmentos: [Segmento::default(); 8], n: 0, inicio: u64::MAX, fin: 0 };
    for k in 0..cuantas {
        let p = desde + k * tam;
        if u32_en(b, p).ok_or("cabeceras de programa cortas")? != 1 {
            continue; // solo PT_LOAD
        }
        let s = Segmento {
            desde: u64_en(b, p + 8).ok_or("cabeceras de programa cortas")? as usize,
            dir: u64_en(b, p + 24).ok_or("cabeceras de programa cortas")?,
            virt: u64_en(b, p + 16).ok_or("cabeceras de programa cortas")?,
            en_fichero: u64_en(b, p + 32).ok_or("cabeceras de programa cortas")?,
            en_memoria: u64_en(b, p + 40).ok_or("cabeceras de programa cortas")?,
        };
        if s.en_fichero > s.en_memoria || b.get(s.desde..s.desde + s.en_fichero as usize).is_none() {
            return Err("un segmento se sale del fichero");
        }
        if e.n == e.segmentos.len() {
            return Err("demasiados segmentos");
        }
        e.segmentos[e.n] = s;
        e.n += 1;
        e.inicio = e.inicio.min(s.dir);
        e.fin = e.fin.max(s.dir + s.en_memoria);
    }
    let segmento = e.segmentos[..e.n].iter().find(|s| (s.virt..s.virt + s.en_memoria).contains(&entrada));
    match segmento {
        Some(s) => e.entrada = entrada - s.virt + s.dir,
        None => return Err("sin segmentos, o la entrada fuera de ellos"),
    }
    Ok(e)
}

/// COM1 (UART 16550 en el puerto 0x3F8), para cuando ya no hay servicios de arranque. El firmware lo deja
/// configurado; aquí solo se espera hueco y se escribe.
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
    const COM1: u16 = 0x3f8;
    for _ in 0..100_000 {
        let lsr: u8;
        unsafe { core::arch::asm!("in al, dx", out("al") lsr, in("dx") COM1 + 5, options(nomem, nostack, preserves_flags)) };
        if lsr & 0x20 != 0 {
            break;
        }
    }
    unsafe { core::arch::asm!("out dx, al", in("dx") COM1, in("al") b, options(nomem, nostack, preserves_flags)) };
}

/// Salta a la entrada del kernel con `info` en `rdi` y las interrupciones cerradas. No vuelve.
fn saltar(entrada: u64, info: u64) -> ! {
    unsafe { core::arch::asm!("cli", "jmp {}", in(reg) entrada, in("rdi") info, options(noreturn)) }
}

#[unsafe(export_name = "efi_main")]
extern "efiapi" fn principal(imagen: *const c_void, st: *const TablaSistema) -> Estado {
    let st = unsafe { &*st };
    let mut c = Consola(st.salida);
    let bs = unsafe { &*st.servicios_arranque };
    let _ = writeln!(c, "DashOS: arranque por UEFI en x86-64 (revisión {:#x} del firmware)", st.cabecera.revision);
    let info = unsafe { &mut *(&raw mut INFO) };
    info.marca = MARCA;
    info.version = VERSION;
    info.rsdp = tabla_config(st, &GUID_ACPI2).or_else(|| tabla_config(st, &GUID_ACPI1)).unwrap_or(0) as u64;
    let _ = writeln!(c, "DashOS: RSDP de ACPI en {:#x}", info.rsdp);
    match buscar_pantalla(bs) {
        Some(p) => {
            let _ = writeln!(c, "DashOS: pantalla del firmware de {}x{} ({} por línea, formato {}) en {:#x}, {} KB", p.ancho, p.alto, p.por_linea, p.formato, p.base, p.tamano / 1024);
            (info.pantalla_base, info.pantalla_tamano) = (p.base, p.tamano);
            (info.pantalla_ancho, info.pantalla_alto, info.pantalla_por_linea, info.pantalla_formato) = (p.ancho, p.alto, p.por_linea, p.formato);
        }
        None => {
            let _ = writeln!(c, "DashOS: el firmware no da pantalla");
        }
    }
    let elf = match leer_elf(NUCLEO) {
        Ok(e) => e,
        Err(motivo) => {
            let _ = writeln!(c, "DashOS: el kernel no vale: {}", motivo);
            return 1;
        }
    };
    // El tramo del kernel, en su dirección física. Si UEFI lo tiene ocupado, no se puede seguir (no es reubicable).
    let mut dir = elf.inicio & !(PAGINA as u64 - 1);
    let paginas = (elf.fin - dir).div_ceil(PAGINA as u64) as usize;
    if (bs.reservar_paginas)(EN_ESA_DIRECCION, CODIGO_DEL_CARGADOR, paginas, &mut dir) != BIEN {
        let _ = writeln!(c, "DashOS: la memoria de {:#x} a {:#x} está ocupada; no se puede cargar el kernel", elf.inicio, elf.fin);
        return 1;
    }
    unsafe { core::ptr::write_bytes(dir as *mut u8, 0, paginas * PAGINA) };
    for s in &elf.segmentos[..elf.n] {
        unsafe { core::ptr::copy_nonoverlapping(NUCLEO.as_ptr().add(s.desde), s.dir as *mut u8, s.en_fichero as usize) };
    }
    let _ = writeln!(c, "DashOS: kernel ELF de {} bytes en {:#x}..{:#x}, entrada en {:#x}", NUCLEO.len(), elf.inicio, elf.fin, elf.entrada);
    // El mapa de memoria y la salida de los servicios de arranque, sin nada en medio que pueda cambiar el mapa. Si
    // cambió (la llave ya no vale), otra vuelta.
    let mapa = &raw mut MAPA;
    for _ in 0..4 {
        let (mut tamano, mut llave, mut tam_desc, mut version) = (unsafe { (*mapa).len() }, 0usize, 0usize, 0u32);
        if (bs.mapa_memoria)(&mut tamano, mapa as *mut u8, &mut llave, &mut tam_desc, &mut version) != BIEN || tam_desc == 0 {
            break;
        }
        (info.mapa, info.mapa_tamano, info.mapa_descriptor, info.mapa_version) = (mapa as u64, tamano as u64, tam_desc as u64, version);
        if (bs.salir_servicios)(imagen, llave) == BIEN {
            // Desde aquí, sin firmware: solo el hardware.
            let _ = writeln!(Serie, "DashOS: fuera de los servicios de arranque; mapa de memoria de {} entradas", tamano / tam_desc);
            saltar(elf.entrada, info as *const InfoArranque as u64);
        }
    }
    let _ = writeln!(c, "DashOS: no se pudo salir de los servicios de arranque");
    1
}
