//! Las IOMMU en x86-64 (X5b): qué unidad lleva cada aparato del PCIe, su dominio y la memoria que el firmware reserva
//! para él. El hardware (registros y tablas) está en `base::iommu`.
//!
//! Al arrancar se encienden todas las unidades de la DMAR (VT-d) o de la IVRS (AMD-Vi) con todos los aparatos
//! bloqueados. Después, cada aparato al que `inicio` da un permiso DMA recibe su dominio (`dominio_para`): su permiso
//! lleva ese dominio y la memoria DMA que se pide con él es lo único que el aparato alcanza (`memoria::PermisoDma`).
//! Los aparatos que el firmware sigue usando por su cuenta (RMRR de la DMAR, IVMD de la IVRS: la pantalla de una gráfica
//! integrada, el USB del firmware) tienen su dominio desde el principio, con esa memoria proyectada 1:1.
//!
//! Un dominio por número de PCI (bus, aparato y función). Lo que no está hecho:
//! - Los alias: un aparato detrás de un puente de PCIe a PCI hace DMA con el número del puente (VT-d, 3.4.5; AMD, las
//!   entradas de alias de la IVRS). Esos aparatos no reciben dominio propio: su DMA falla. En `q35` no los hay.
//! - Los grupos: dos funciones de un aparato sin ACS pueden hablarse sin pasar por la IOMMU. Cada una tiene aquí su
//!   dominio igualmente.
//! - Solo el segmento 0 del PCI.
//!
//! El remapeo de interrupciones de VT-d (X11b): si la DMAR lo admite (INTR_REMAP) y la unidad también (ECAP.IR), se
//! enciende en cada unidad que lo tenga, siempre que cada IOAPIC de la MADT salga en el ámbito de alguna unidad (su
//! número de PCI hace falta para comprobar el origen de sus mensajes). Desde ahí, las MSI de los aparatos se programan
//! con `dar_msi` (formato remapeable, con su entrada) y los IOAPIC con `base::ioapic` (igual). Sin remapeo, todo va en
//! formato compatible, como antes.

use crate::base::acpi::{Acpi, Ambito, Iommu, ParaQuien};
use crate::base::iommu as hw;
use crate::cerrojo::Cerrojo;
use crate::kprintln;
use crate::pci::Funcion;
use alloc::vec::Vec;

pub use hw::Dominio;

/// Lo que lleva una unidad, ya resuelto con el PCIe.
struct Cobertura {
    unidad: usize,
    /// VT-d con INCLUDE_PCI_ALL, o AMD con "todos": lo que no lleve otra.
    todos: bool,
    aparatos: Vec<u16>,
    /// Buses enteros (los que cuelgan de un puente del ámbito).
    buses: Vec<(u8, u8)>,
    rangos: Vec<(u16, u16)>,
}

static COBERTURAS: Cerrojo<Vec<Cobertura>> = Cerrojo::new(Vec::new());

/// El número de PCI de una función: bus (15:8), aparato (7:3) y función (2:0).
pub fn numero(f: &Funcion) -> u16 {
    (f.bus as u16) << 8 | (f.aparato as u16) << 3 | f.funcion as u16
}

fn texto(n: u16) -> (u16, u16, u16) {
    (n >> 8, (n >> 3) & 0x1f, n & 7)
}

fn buscar(funciones: &[Funcion], bus: u8, aparato: u8, funcion: u8) -> Option<&Funcion> {
    funciones.iter().find(|f| f.bus == bus && f.aparato == aparato && f.funcion == funcion)
}

/// Resuelve un ámbito de la DMAR (VT-d, 8.3.1.1): sigue el camino por los puentes (su bus secundario, el byte 0x19 de
/// la configuración) hasta el aparato. Devuelve su número y, si es un puente (tipo 2), sus buses secundario y
/// subordinado (bytes 0x19 y 0x1A).
fn resolver(a: &Ambito, funciones: &[Funcion]) -> Option<(u16, Option<(u8, u8)>)> {
    let mut bus = a.bus;
    for (k, &(d, f)) in a.camino.iter().enumerate() {
        let ultimo = k + 1 == a.camino.len();
        if ultimo && a.tipo != 2 {
            return Some(((bus as u16) << 8 | (d as u16) << 3 | f as u16, None));
        }
        let r = buscar(funciones, bus, d, f)?.leer(0x18);
        if ultimo {
            return Some(((bus as u16) << 8 | (d as u16) << 3 | f as u16, Some(((r >> 8) as u8, (r >> 16) as u8))));
        }
        bus = (r >> 8) as u8;
    }
    None
}

/// Los aparatos (endpoints, tipo 1) y los buses (puentes, tipo 2) de unos ámbitos.
fn aparatos_de(ambitos: &[Ambito], funciones: &[Funcion]) -> (Vec<u16>, Vec<(u8, u8)>) {
    let (mut aparatos, mut buses) = (Vec::new(), Vec::new());
    for a in ambitos.iter().filter(|a| a.tipo == 1 || a.tipo == 2) {
        match resolver(a, funciones) {
            Some((n, None)) => aparatos.push(n),
            Some((n, Some(b))) => {
                aparatos.push(n);
                buses.push(b);
            }
            None => {}
        }
    }
    (aparatos, buses)
}

/// La unidad que lleva el aparato `n`, si alguna lo lleva: la que lo nombra; si ninguna, la de "todos".
fn unidad_de(n: u16) -> Option<usize> {
    let c = COBERTURAS.tomar();
    let bus = (n >> 8) as u8;
    let nombra = |c: &Cobertura| {
        c.aparatos.contains(&n) || c.buses.iter().any(|&(d, h)| (d..=h).contains(&bus)) || c.rangos.iter().any(|&(d, h)| (d..=h).contains(&n))
    };
    c.iter().find(|c| nombra(c)).or_else(|| c.iter().find(|c| c.todos)).map(|c| c.unidad)
}

/// Enciende las IOMMU de ACPI, si las hay, con todos los aparatos bloqueados, y da su dominio a los que tienen memoria
/// reservada por el firmware. Sin DMAR ni IVRS, nada cambia: el DMA de los aparatos no se limita.
pub fn iniciar(a: &Acpi, funciones: &[Funcion]) {
    if a.iommus.is_empty() {
        kprintln!("IOMMU: no hay (ni DMAR ni IVRS); el DMA de los aparatos no se limita");
        return;
    }
    // Las unidades de VT-d encendidas, con sus ámbitos (para el remapeo de interrupciones: los IOAPIC).
    let mut intel: Vec<(usize, u64, &[Ambito])> = Vec::new();
    for i in &a.iommus {
        match i {
            Iommu::Intel { base, segmento, todos, ambitos } => {
                if *segmento != 0 {
                    kprintln!("IOMMU: la VT-d en {:#x} es del segmento {} del PCI; no se usa", base, segmento);
                    continue;
                }
                match hw::encender_intel(*base) {
                    Ok(u) => {
                        let (aparatos, buses) = aparatos_de(ambitos, funciones);
                        kprintln!(
                            "IOMMU: VT-d en {:#x}, traducción encendida ({} niveles); lleva {}",
                            base,
                            hw::unidades()[u].2,
                            if *todos { alloc::format!("todo el PCI") } else { alloc::format!("{} aparatos y {} puentes", aparatos.len(), buses.len()) }
                        );
                        COBERTURAS.tomar().push(Cobertura { unidad: u, todos: *todos, aparatos, buses, rangos: Vec::new() });
                        intel.push((u, *base, ambitos));
                    }
                    Err(e) => kprintln!("IOMMU: la VT-d en {:#x} no se enciende: {}", base, e),
                }
            }
            Iommu::Amd { base, segmento, propio, todos, rangos } => {
                if *segmento != 0 {
                    kprintln!("IOMMU: la AMD-Vi en {:#x} es del segmento {} del PCI; no se usa", base, segmento);
                    continue;
                }
                match hw::encender_amd(*base) {
                    Ok(u) => {
                        // Su aviso: la MSI de la propia IOMMU, en su configuración del PCI (AMD, 3.2).
                        let msi = funciones
                            .iter()
                            .find(|f| numero(f) == *propio)
                            .and_then(|f| hw::mensaje_de_aviso().and_then(|(d, v)| f.dar_msi(d, v)));
                        COBERTURAS.tomar().push(Cobertura { unidad: u, todos: *todos, aparatos: Vec::new(), buses: Vec::new(), rangos: rangos.clone() });
                        // Que relea la entrada (bloqueada) de cada aparato que lleva.
                        for f in funciones.iter().filter(|f| unidad_de(numero(f)) == Some(u)) {
                            hw::bloquear(u, numero(f));
                        }
                        let (b, d, f) = texto(*propio);
                        kprintln!(
                            "IOMMU: AMD-Vi en {:#x} ({}:{}.{}), traducción encendida ({} niveles); lleva {}; avisos {}",
                            base,
                            b,
                            d,
                            f,
                            hw::unidades()[u].2,
                            if *todos { alloc::format!("todo el PCI") } else { alloc::format!("{} rangos de aparatos", rangos.len()) },
                            match msi {
                                Some(como) => alloc::format!("por su {}", como),
                                None => alloc::format!("sin interrupción (no se encuentra su MSI)"),
                            }
                        );
                    }
                    Err(e) => kprintln!("IOMMU: la AMD-Vi en {:#x} no se enciende: {}", base, e),
                }
            }
        }
    }
    if !hw::activa() {
        return;
    }
    if !intel.is_empty() {
        remapeo_intel(a, &intel, funciones);
    }
    // La memoria que el firmware reserva para algunos aparatos, 1:1 en su dominio.
    for r in &a.reservas_dma {
        let (inicio, fin) = (r.inicio & !0xfff, r.fin.next_multiple_of(0x1000));
        let mut suyos: Vec<u16> = match &r.para {
            ParaQuien::Ambitos(ambitos) => {
                let (mut aparatos, buses) = aparatos_de(ambitos, funciones);
                for f in funciones.iter().filter(|f| buses.iter().any(|&(d, h)| (d..=h).contains(&f.bus))) {
                    aparatos.push(numero(f));
                }
                aparatos
            }
            ParaQuien::Rango(d, h) => funciones.iter().map(numero).filter(|n| (*d..=*h).contains(n)).collect(),
            ParaQuien::Todos => funciones.iter().map(numero).collect(),
        };
        suyos.sort_unstable();
        suyos.dedup();
        for n in suyos {
            let (b, d, f) = texto(n);
            let Some(u) = unidad_de(n) else { continue };
            let hecho = hw::dominio(u, n).is_some_and(|dom| dom.proyectar(inicio, inicio, ((fin - inicio) / 0x1000) as usize, true));
            kprintln!("IOMMU: {}:{}.{} con su memoria del firmware, {:#x} a {:#x}: {}", b, d, f, inicio, fin, if hecho { "proyectada" } else { "NO se pudo proyectar" });
        }
    }
    // Lo que hubiera pasado mientras (aparatos que el firmware dejó haciendo DMA).
    hw::atender();
}

/// VT-d: enciende el remapeo de interrupciones en las unidades que lo admiten, si la DMAR lo admite y cada IOAPIC de la
/// MADT sale en el ámbito de alguna unidad (si no, no se sabría con qué número de PCI manda sus mensajes, y en una
/// unidad con remapeo se bloquearían). Luego vuelve a programar lo que ya estuviera en marcha en los IOAPIC.
fn remapeo_intel(a: &Acpi, intel: &[(usize, u64, &[Ambito])], funciones: &[Funcion]) {
    let con: Vec<usize> = intel.iter().map(|x| x.0).filter(|&u| hw::puede_remapear(u)).collect();
    if con.is_empty() {
        kprintln!("IOMMU: VT-d sin remapeo de interrupciones (la unidad no lo tiene); van en formato compatible");
        return;
    }
    if !a.dmar_remapeo {
        kprintln!("IOMMU: VT-d sin remapeo de interrupciones (la DMAR dice que la plataforma no lo admite)");
        return;
    }
    // Cada IOAPIC: la unidad que lo nombra (ámbito de tipo 3 con su ID) y su número de PCI.
    let mut ioapics: Vec<(u8, u16)> = Vec::new();
    for io in &a.ioapics {
        let suyo = intel.iter().find_map(|&(u, _, ambitos)| {
            ambitos.iter().find(|x| x.tipo == 3 && x.id == io.id).and_then(|x| resolver(x, funciones)).map(|(n, _)| (u, n))
        });
        match suyo {
            Some((u, n)) if con.contains(&u) => ioapics.push((io.id, n)),
            Some(_) => {}
            None => {
                kprintln!("IOMMU: VT-d sin remapeo de interrupciones: el IOAPIC {} no sale en la DMAR", io.id);
                return;
            }
        }
    }
    match hw::encender_remapeo(&con, &ioapics) {
        Ok(()) => {
            crate::base::ioapic::rehacer();
            let eime = hw::remapeo() == Some(true);
            for &(u, base, _) in intel.iter().filter(|x| con.contains(&x.0)) {
                let suyos = ioapics.iter().filter(|io| intel.iter().any(|&(v, _, amb)| v == u && amb.iter().any(|x| x.tipo == 3 && x.id == io.0))).count();
                kprintln!(
                    "IOMMU: VT-d en {:#x}, remapeo de interrupciones encendido ({} entradas, destinos de {} bits; {} IOAPIC); el formato compatible, bloqueado",
                    base,
                    hw::ENTRADAS_IR,
                    if eime { 32 } else { 8 },
                    suyos
                );
            }
        }
        Err(e) => kprintln!("IOMMU: VT-d, el remapeo de interrupciones no se enciende: {}", e),
    }
}

/// Da a `f` la interrupción por mensaje del anfitrión que dice `interrupciones::msi` (dirección en formato compatible,
/// con el ID de APIC del destino en 19:12, y el dato con el vector). Si la unidad de VT-d que lleva `f` remapea, le
/// pone su entrada en la tabla (solo `f` la puede usar) y le programa el mensaje en formato remapeable; si no, el
/// mensaje tal cual. Como `Funcion::dar_msi`: qué se usó (MSI o MSI-X), o `None` si no tiene, o no hay entrada libre.
pub fn dar_msi(f: &Funcion, direccion: u64, dato: u32) -> Option<&'static str> {
    let n = numero(f);
    match unidad_de(n).filter(|&u| hw::remapea(u)) {
        Some(_) => {
            let (destino, vector) = (((direccion >> 12) & 0xff) as u32, dato as u8);
            let Some(i) = hw::irte_poner(n, hw::CLAVE_MSI | vector as u32, destino, vector, false) else {
                let (b, d, fu) = texto(n);
                kprintln!("IOMMU: sin entrada de remapeo para la MSI de {}:{}.{}", b, d, fu);
                return None;
            };
            f.dar_msi(hw::direccion_remapeada(i), 0)
        }
        None => f.dar_msi(direccion, dato),
    }
}

/// El dominio de un aparato al que se da un permiso DMA: `Ok(None)` si no hay IOMMU o ninguna lo lleva (su DMA no se
/// limita); `Err` si hay una que lo lleva pero no se le pudo hacer dominio (sin memoria, sin números de dominio).
pub fn dominio_para(f: &Funcion) -> Result<Option<Dominio>, ()> {
    if !hw::activa() {
        return Ok(None);
    }
    let n = numero(f);
    match unidad_de(n) {
        None => Ok(None),
        Some(u) => hw::dominio(u, n).map(Some).ok_or(()),
    }
}

/// Hay alguna IOMMU encendida.
pub fn activa() -> bool {
    hw::activa()
}
