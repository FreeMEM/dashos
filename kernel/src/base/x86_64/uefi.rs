//! S32: las variables del firmware (la NVRAM de UEFI) con sus servicios de ejecución, para que el instalador ponga la
//! entrada de DashOS en el menú de arranque, como hace cualquier sistema (UEFI 2.10, 8.2: GetVariable,
//! GetNextVariableName, SetVariable; 3.3: las variables de arranque).
//!
//! No se llama a SetVirtualAddressMap: los servicios de ejecución siguen en sus direcciones físicas. Para llamarlos se
//! usa un PML4 aparte, solo para esto: la mitad alta, la del kernel (la entrada 511, la misma de siempre), y en la mitad
//! baja, 1:1, solo las zonas del mapa de UEFI que el firmware marca como de ejecución (EFI_MEMORY_RUNTIME): su código,
//! sus datos y los registros que usa (la flash de las variables). Su código va con escritura y ejecución a la vez: el
//! firmware de EDK2 pone el `.data` de sus controladores de ejecución en zonas de código (sin la tabla de atributos de
//! memoria, no se sabe qué parte es cuál); solo existe durante la llamada, en este núcleo.
//!
//! La llamada: con el cerrojo (interrupciones cerradas, una a la vez en toda la máquina), la FPU libre y guardada
//! (el firmware usa SSE), una pila propia de 64 KB, el CR3 de este PML4; a la vuelta, todo como estaba y la TLB
//! vaciada entera (CR4.PGE), también lo global.

use super::cerrojo::Cerrojo;
use super::{cpu, mmu};
use crate::memoria::{paginas, Zona, PAGINA};
use alloc::vec::Vec;
use core::arch::asm;

const PRESENTE: u64 = 1;
const ESCRITURA: u64 = 1 << 1;
const PWT: u64 = 1 << 3;
const PCD: u64 = 1 << 4;
const NX: u64 = 1 << 63;
const DIRECCION: u64 = 0x000f_ffff_ffff_f000;
/// EFI_MEMORY_RUNTIME, en los atributos de un descriptor del mapa de memoria.
const RUNTIME: u64 = 1 << 63;
/// Las de "sin caché" (UC, WC, WT) de los atributos.
const SIN_CACHE: u64 = 1 | 2 | 4;

/// Estado de una llamada (UEFI 2.10, apéndice D): 0 bien; con el bit alto, error.
pub type Estado = u64;
pub const BIEN: Estado = 0;
pub const NO_ENCONTRADO: Estado = (1 << 63) | 14;
pub const DEMASIADO_PEQUENO: Estado = (1 << 63) | 5;
/// Lo que da `variable` si no hay servicios de ejecución o no se pudo preparar la llamada.
pub const NO_HAY: Estado = (1 << 63) | 3;

struct Llamadas {
    pml4: u64,
    tabla: u64,
    pila: Vec<u8>,
    fp: Vec<u8>,
}

static UEFI: Cerrojo<Option<Llamadas>> = Cerrojo::new(None);

fn tabla_nueva() -> Option<u64> {
    let t = paginas().pagina(Zona::Normal)?;
    unsafe { core::ptr::write_bytes(mmu::virt(t as usize) as *mut u8, 0, PAGINA as usize) };
    Some(t)
}

fn leer(t: u64, i: usize) -> u64 {
    unsafe { ((mmu::virt(t as usize) + 8 * i) as *const u64).read_volatile() }
}

fn poner(t: u64, i: usize, v: u64) {
    unsafe { ((mmu::virt(t as usize) + 8 * i) as *mut u64).write_volatile(v) };
}

/// Proyecta la página física `f` en la virtual `f` (1:1) del PML4 `pml4`, con `atr`.
fn proyectar(pml4: u64, f: u64, atr: u64) -> Option<()> {
    let mut t = pml4;
    for nivel in [39, 30, 21] {
        let i = ((f >> nivel) & 511) as usize;
        let e = leer(t, i);
        t = if e & PRESENTE != 0 {
            e & DIRECCION
        } else {
            let n = tabla_nueva()?;
            poner(t, i, n | PRESENTE | ESCRITURA);
            n
        };
    }
    poner(t, ((f >> 12) & 511) as usize, (f & DIRECCION) | atr);
    Some(())
}

/// Prepara el PML4 de las llamadas (una vez). `None` si no hay servicios de ejecución.
fn preparar() -> Option<Llamadas> {
    let tabla = super::arranque::ejecucion()?;
    let pml4 = tabla_nueva()?;
    // La mitad alta: la misma entrada 511 del kernel.
    poner(pml4, 511, leer(super::mapa::pml4(), 511));
    let mut zonas = 0;
    for z in super::arranque::mapa_con_atributos() {
        if z.atributos & RUNTIME == 0 || z.paginas == 0 || z.inicio >= 1 << 47 {
            continue;
        }
        // 5: código de ejecución (con ejecución); 11 y 12: registros y puertos de los aparatos (sin caché); lo demás,
        // datos (sin ejecución).
        let atr = match z.tipo {
            5 => PRESENTE | ESCRITURA,
            11 | 12 => PRESENTE | ESCRITURA | PCD | PWT | NX,
            _ if z.atributos & 8 == 0 && z.atributos & SIN_CACHE != 0 => PRESENTE | ESCRITURA | PCD | PWT | NX,
            _ => PRESENTE | ESCRITURA | NX,
        };
        for k in 0..z.paginas.min(1 << 20) {
            proyectar(pml4, z.inicio + k * PAGINA, atr)?;
        }
        zonas += 1;
    }
    // La tabla misma tiene que verse (está en datos de ejecución; si no, nada).
    let rt = tabla as usize;
    if !super::mapa::se_ve(tabla, 128) || zonas == 0 {
        return None;
    }
    let firma = unsafe { core::ptr::read_volatile(mmu::virt(rt) as *const u64) };
    // "RUNTSERV" (EFI_RUNTIME_SERVICES_SIGNATURE).
    if firma != 0x5652_4553_544E_5552 {
        crate::kprintln!("uefi: la tabla de servicios de ejecución no tiene su firma");
        return None;
    }
    crate::kprintln!("uefi: servicios de ejecución en {:#x}, {} zonas proyectadas 1:1 para llamarlos", tabla, zonas);
    let mut fp = Vec::new();
    fp.resize(cpu::TAMANO_FP + 64, 0u8);
    let mut pila = Vec::new();
    pila.resize(64 * 1024, 0u8);
    Some(Llamadas { pml4, tabla, pila, fp })
}

/// Llama a la función `n` (8 bytes cada una tras la cabecera de 24) de la tabla con cinco argumentos.
fn llamar(e: &mut Llamadas, n: usize, a: [u64; 5]) -> Estado {
    let f = unsafe { core::ptr::read_volatile((mmu::virt(e.tabla as usize) + 24 + 8 * n) as *const u64) };
    let fp = (e.fp.as_mut_ptr() as usize).next_multiple_of(64) as *mut u8;
    let pila = (e.pila.as_ptr() as usize + e.pila.len()) & !15;
    let cr0 = cpu::cr0();
    cpu::fpu_libre(true);
    unsafe { cpu::fp_guardar(fp) };
    let cr3 = cpu::cr3();
    let r: u64;
    unsafe {
        // ms_abi: rcx, rdx, r8, r9 y el quinto en la pila, tras los 32 bytes de sombra. Pila propia alineada a 16.
        asm!(
            "mov r12, rsp",
            "mov rsp, {pila}",
            "sub rsp, 48",
            "mov [rsp + 32], {a4}",
            "mov cr3, {nuevo}",
            "call {f}",
            "mov cr3, r13",
            "mov rsp, r12",
            pila = in(reg) pila,
            a4 = in(reg) a[4],
            nuevo = in(reg) e.pml4,
            f = in(reg) f,
            in("rcx") a[0],
            in("rdx") a[1],
            in("r8") a[2],
            in("r9") a[3],
            in("r13") cr3,
            out("r12") _,
            lateout("rax") r,
            clobber_abi("win64"),
        );
    }
    // Toda la TLB fuera (también lo global): la mitad baja tenía otra cosa.
    let cr4 = cpu::cr4();
    cpu::poner_cr4(cr4 & !(1 << 7));
    cpu::poner_cr4(cr4);
    unsafe { cpu::fp_cargar(fp) };
    if cr0 & (1 << 3) != 0 {
        cpu::fpu_libre(false);
    }
    r
}

/// Qué hacer con una variable.
pub enum Operacion<'a> {
    /// Leer `nombre` (UTF-16 acabado en 0) de `guid` en `datos`: (estado, atributos, bytes).
    Leer,
    /// Escribir `datos` con esos atributos (sin datos: borrarla).
    Escribir(u32, &'a [u8]),
    /// La siguiente tras (`nombre`, `guid`): deja su nombre en `datos` (UTF-16) y su GUID en `guid`.
    Siguiente,
}

/// Una operación con una variable del firmware. Devuelve (estado, atributos, bytes de datos o del nombre).
pub fn variable(op: Operacion, nombre: &[u16], guid: &mut [u8; 16], datos: &mut [u8]) -> (Estado, u32, usize) {
    let mut g = UEFI.tomar();
    if g.is_none() {
        *g = preparar();
    }
    let Some(e) = g.as_mut() else { return (NO_HAY, 0, 0) };
    // Todo lo que se pasa, en memoria del kernel (mitad alta, se ve en el PML4 de la llamada).
    let mut n: Vec<u16> = nombre.to_vec();
    match op {
        Operacion::Leer => {
            let mut atr = 0u32;
            let mut tam = datos.len();
            let r = llamar(e, 6, [n.as_ptr() as u64, guid.as_ptr() as u64, &mut atr as *mut u32 as u64, &mut tam as *mut usize as u64, datos.as_mut_ptr() as u64]);
            (r, atr, tam)
        }
        Operacion::Escribir(atr, d) => {
            let copia: Vec<u8> = d.to_vec();
            let r = llamar(e, 8, [n.as_ptr() as u64, guid.as_ptr() as u64, atr as u64, copia.len() as u64, copia.as_ptr() as u64]);
            (r, atr, 0)
        }
        Operacion::Siguiente => {
            // El nombre entra y sale por el mismo búfer, del tamaño de `datos`.
            n.resize(datos.len() / 2, 0);
            let mut tam = n.len() * 2;
            let r = llamar(e, 7, [&mut tam as *mut usize as u64, n.as_mut_ptr() as u64, guid.as_mut_ptr() as u64, 0, 0]);
            if r == BIEN {
                for (i, c) in n.iter().take(datos.len() / 2).enumerate() {
                    datos[2 * i..2 * i + 2].copy_from_slice(&c.to_le_bytes());
                }
            }
            (r, 0, tam)
        }
    }
}
