//! El programa de inicio en x86-64 (X5): como `inicio.rs` de ARM (C47), con el mismo paquete de arranque (formato
//! allí), los ELF de x86 y las llaves de los aparatos que salen de ACPI y del PCIe en vez del árbol de dispositivos.
//!
//! Lo que hay en x86 por ahora: el xHCI, el NVMe y el sonido de virtio, cada uno con sus registros (su BAR), un permiso
//! DMA propio y una MSI (MSI-X o MSI) si la tiene, y los servicios que van encima (disco, archivos, sonido). Sin consola
//! (el kernel escribe en COM1, que es de E/S por puertos y un proceso no llega) ni compositor (la pantalla es X6): el
//! xHCI manda sus eventos a `entrada`, un programa de prueba que los escribe.
//!
//! Sin IOMMU: el permiso DMA da memoria de la zona baja (por debajo de 4 GB), pero un aparato con el bus master
//! encendido llega a toda la memoria física (`docs/referencia/kernel.md`, limitaciones).

use crate::objetos::{Llave, Objeto, LEER};
use crate::pci::Funcion;
use crate::{interrupciones, kprintln, memoria};
use alloc::vec::Vec;

macro_rules! elf {
    ($n:literal) => {
        include_bytes!(concat!("../../usuario/target/x86_64-unknown-none/release/", $n))
    };
}

static INICIO: &[u8] = elf!("inicio");

/// Los servicios que lanza `inicio`, en orden, con los aparatos (por nombre y tipo) y los canales (por nombre) que le
/// pasa a cada uno: los de ARM (`inicio.rs`) que tienen sus aparatos en un PC.
static PROGRAMAS: &[(&str, &[u8], &[(&str, u32)], &[&str])] = &[
    // Quien lee los eventos del teclado y el ratón (en ARM, el compositor): nombra "ventanas" antes que el xHCI.
    ("entrada", elf!("entrada"), &[], &["ventanas"]),
    ("xhci", elf!("xhci"), &[("dma-xhci", PERMISO_DMA), ("xhci", REGISTROS), ("xhci", INTERRUPCION | OPCIONAL)], &["ventanas"]),
    ("nvme", elf!("nvme"), &[("dma-nvme", PERMISO_DMA), ("nvme", REGISTROS), ("nvme", INTERRUPCION | OPCIONAL)], &["disco"]),
    ("disco", elf!("disco"), &[], &["disco"]),
    ("archivos", elf!("archivos"), &[], &["disco", "archivos"]),
    ("ficheros", elf!("ficheros"), &[], &["archivos"]),
    ("sonido", elf!("sonido"), &[], &["sonido"]),
    ("virtio-sonido", elf!("virtio-sonido"), &[("dma-vsonido", PERMISO_DMA), ("virtio-sonido", REGISTROS), ("vsonido-cfg", REGISTROS), ("virtio-sonido", INTERRUPCION | OPCIONAL)], &["sonido"]),
    ("tono-a", elf!("tono-a"), &[], &["sonido"]),
    ("tono-b", elf!("tono-b"), &[], &["sonido"]),
    ("hola", elf!("hola"), &[], &[]),
];

const REGISTROS: u32 = 1;
const INTERRUPCION: u32 = 2;
const PERMISO_DMA: u32 = 3;
const OPCIONAL: u32 = 0x100;
const DERECHOS: u32 = crate::objetos::TODOS;

fn nombre16(n: &str) -> [u8; 16] {
    let mut b = [0u8; 16];
    let l = n.len().min(16);
    b[..l].copy_from_slice(&n.as_bytes()[..l]);
    b
}

/// Los aparatos del PCIe que se dan a `inicio`: nombre, cómo se reconoce (clase o fabricante y aparato), la BAR de sus
/// registros, si se le da también su configuración y el nombre de su permiso DMA. Las clases: xHCI, 0x0C0330
/// (especificación de xHCI); NVMe, 0x010802 (especificación de NVMe). El sonido de virtio: virtio 1.2, 4.1.2,
/// fabricante 0x1af4 y aparato 0x1040 más el tipo, 25; en QEMU, todos sus registros en la BAR 4.
enum Quien {
    Clase(u32),
    Id(u16, u16),
}
const APARATOS: &[(&str, Quien, usize, Option<&str>, &str)] = &[
    ("virtio-sonido", Quien::Id(0x1af4, 0x1059), 4, Some("vsonido-cfg"), "dma-vsonido"),
    ("xhci", Quien::Clase(0x0C_0330), 0, None, "dma-xhci"),
    ("nvme", Quien::Clase(0x01_0802), 0, None, "dma-nvme"),
];

/// Prepara un aparato: su BAR (en páginas enteras), el bus master y una MSI. (física, tamaño, interrupción).
fn preparar(f: &Funcion, bar: usize, nombre: &str) -> Option<(u64, u64, Option<u32>)> {
    let Some((base, tamano)) = f.bar(bar) else {
        kprintln!("{}: en el PCIe {}:{}.{}, sin BAR {} de memoria", nombre, f.bus, f.aparato, f.funcion, bar);
        return None;
    };
    let paginas = tamano.max(memoria::PAGINA).div_ceil(memoria::PAGINA) * memoria::PAGINA;
    if !memoria::paginas().fuera_de_la_ram(base, paginas) {
        kprintln!("{}: BAR en {:#x} dentro de la RAM", nombre, base);
        return None;
    }
    f.encender(false);
    let irq = interrupciones::msi().and_then(|(intid, direccion, dato)| f.dar_msi(direccion, dato).map(|como| (intid, como)));
    match irq {
        Some((i, como)) => kprintln!("{}: en el PCIe {}:{}.{}, registros en {:#x} ({} KB), {} en la {}", nombre, f.bus, f.aparato, f.funcion, base, tamano / 1024, como, i),
        None => kprintln!("{}: en el PCIe {}:{}.{}, registros en {:#x} ({} KB), sin MSI", nombre, f.bus, f.aparato, f.funcion, base, tamano / 1024),
    }
    Some((base, paginas, irq.map(|(i, _)| i)))
}

/// Arma las llaves de arranque y lanza `inicio`. Falso si no se pudo.
pub fn lanzar(a: &crate::base::acpi::Acpi) -> bool {
    let mut llaves: Vec<Llave> = Vec::new();
    let mut manifiesto: Vec<([u8; 16], u32, u32)> = Vec::new();
    // La llave 1 es el paquete; las de los aparatos empiezan en la 2.
    let mut apuntar = |llaves: &mut Vec<Llave>, nombre: &str, tipo: u32, l: Llave| {
        llaves.push(l);
        manifiesto.push((nombre16(nombre), tipo, llaves.len() as u32 + 1));
    };
    let funciones = crate::pci::enumerar(a);
    kprintln!("PCIe: {} tramos de ECAM en la MCFG, {} funciones", a.ecam.len(), funciones.len());
    for (nombre, quien, bar, cfg, dma) in APARATOS {
        let Some(f) = funciones.iter().find(|f| match quien {
            Quien::Clase(c) => f.clase == *c,
            Quien::Id(fab, ap) => f.id == (*ap as u32) << 16 | *fab as u32,
        }) else {
            continue;
        };
        let Some((fisica, tamano, irq)) = preparar(f, *bar, nombre) else { continue };
        apuntar(&mut llaves, nombre, REGISTROS, Llave::nueva(Objeto::Registros { fisica, tamano }, DERECHOS));
        if let Some(c) = cfg {
            apuntar(&mut llaves, c, REGISTROS, Llave::nueva(Objeto::Registros { fisica: f.fisica_cfg, tamano: memoria::PAGINA }, DERECHOS));
        }
        if let Some(irq) = irq.and_then(interrupciones::Interrupcion::nueva) {
            apuntar(&mut llaves, nombre, INTERRUPCION, Llave::nueva(Objeto::Interrupcion(irq), DERECHOS));
        }
        let permiso = Objeto::PermisoDma(alloc::sync::Arc::new(memoria::PermisoDma::nuevo(memoria::Zona::Dma1G, 0)));
        apuntar(&mut llaves, dma, PERMISO_DMA, Llave::nueva(permiso, DERECHOS));
    }
    // Un servicio al que le falta algún aparato no se lanza.
    let lanzables: Vec<_> = PROGRAMAS
        .iter()
        .filter(|(nombre, _, aparatos, _)| {
            let falta = aparatos.iter().find(|(a, t)| t & OPCIONAL == 0 && !manifiesto.iter().any(|(n, tm, _)| *n == nombre16(a) && tm == t));
            if let Some((a, _)) = falta {
                kprintln!("inicio: {} no se lanza: falta {}", nombre, a);
            }
            falta.is_none()
        })
        .collect();
    // El paquete, con el formato de `inicio.rs`.
    let mut paquete: Vec<u8> = Vec::new();
    paquete.extend_from_slice(&(manifiesto.len() as u32).to_le_bytes());
    for (nombre, tipo, llave) in &manifiesto {
        paquete.extend_from_slice(nombre);
        paquete.extend_from_slice(&tipo.to_le_bytes());
        paquete.extend_from_slice(&llave.to_le_bytes());
    }
    paquete.extend_from_slice(&(lanzables.len() as u32).to_le_bytes());
    let mut canales: Vec<&str> = Vec::new();
    for (nombre, elf, aparatos, suyos) in lanzables.iter() {
        let mut suyos_aparatos = [0xFFu8; 8];
        for (k, (aparato, tipo)) in aparatos.iter().enumerate().take(8) {
            if let Some(i) = manifiesto.iter().position(|(n, t, _)| *n == nombre16(aparato) && *t == tipo & !OPCIONAL) {
                suyos_aparatos[k] = i as u8;
            }
        }
        let mut de_canales = 0u32;
        for c in suyos.iter() {
            let i = canales.iter().position(|x| x == c).unwrap_or_else(|| {
                canales.push(c);
                canales.len() - 1
            });
            de_canales |= 1 << i;
        }
        paquete.extend_from_slice(&(elf.len() as u32).to_le_bytes());
        paquete.extend_from_slice(&de_canales.to_le_bytes());
        paquete.extend_from_slice(&suyos_aparatos);
        paquete.extend_from_slice(&nombre16(nombre));
    }
    for (_, elf, _, _) in lanzables.iter() {
        while paquete.len() % 8 != 0 {
            paquete.push(0);
        }
        paquete.extend_from_slice(elf);
    }
    let raiz = crate::cupo::Cupo::raiz();
    let Some(m) = memoria::Memoria::nueva(paquete.len(), &raiz) else { return false };
    m.escribir(0, &paquete);
    llaves.insert(0, Llave::nueva(Objeto::Memoria(alloc::sync::Arc::new(m)), LEER));
    match crate::procesos::lanzar(INICIO, llaves, Some(1), raiz) {
        Ok(_) => true,
        Err((e, _)) => {
            kprintln!("no se pudo lanzar inicio: {:?}", e);
            false
        }
    }
}
