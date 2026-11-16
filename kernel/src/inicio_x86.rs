//! El programa de inicio en x86-64 (X5): como `inicio.rs` de ARM (C47), con el mismo paquete de arranque (formato
//! allí), los ELF de x86 y las llaves de los aparatos que salen de ACPI y del PCIe en vez del árbol de dispositivos.
//!
//! Lo que hay en x86 por ahora: el xHCI, el NVMe y el sonido de virtio, cada uno con sus registros (su BAR), un permiso
//! DMA propio y una MSI (MSI-X o MSI) si la tiene, y los servicios que van encima (disco, archivos, sonido). Y (X6) la
//! pantalla del GOP para el compositor, que lee el teclado y el ratón del xHCI, con las ventanas de prueba y Ajustes.
//! La consola es un proceso, como en ARM: recibe el registro y los puertos de COM1 (una llave de puertos, que se usa con
//! `puertos_permitir` y queda en el mapa de E/S de la TSS).
//!
//! El permiso DMA da memoria de la zona baja (por debajo de 4 GB). Sin IOMMU, un aparato con el bus master encendido
//! llega a toda la memoria física (`docs/referencia/kernel.md`, limitaciones). Con IOMMU (X5b), el permiso lleva el
//! dominio del aparato y solo alcanza la memoria DMA pedida con él (`iommu.rs`).

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
    // La consola (C48), con el registro y los puertos de COM1 (X6).
    ("consola", elf!("consola"), &[("registro", REGISTRO), ("serie", PUERTOS)], &[]),
    // El compositor (X6), con la pantalla del GOP: nombra "ventanas" antes que el xHCI y que los clientes de Wayland,
    // así es quien lee (el servidor), como en ARM.
    ("compositor", elf!("compositor"), &[("pantalla", FIRMWARE), ("pantalla-info", DATOS)], &["ventanas"]),
    ("xhci", elf!("xhci"), &[("dma-xhci", PERMISO_DMA), ("xhci", REGISTROS), ("xhci", INTERRUPCION | OPCIONAL)], &["ventanas"]),
    ("nvme", elf!("nvme"), &[("dma-nvme", PERMISO_DMA), ("nvme", REGISTROS), ("nvme", INTERRUPCION | OPCIONAL)], &["disco"]),
    // X5c: los discos SATA por AHCI. Su canal es "sata" si hay NVMe (que sirve "disco"); si no, "disco" (`canal`), y
    // los demás servicios del disco van por él: por eso va antes que `disco` (el primero que nombra un canal
    // es quien lo sirve). `disco-sata`, su prueba.
    ("ahci", elf!("ahci"), &[("dma-ahci", PERMISO_DMA), ("ahci", REGISTROS), ("ahci", INTERRUPCION | OPCIONAL)], &["sata"]),
    ("disco-sata", elf!("disco-sata"), &[], &["sata"]),
    ("disco", elf!("disco"), &[], &["disco"]),
    ("archivos", elf!("archivos"), &[], &["disco", "archivos"]),
    ("ficheros", elf!("ficheros"), &[], &["archivos"]),
    // Desde la fusión con `revision`, el sonido y su controlador reciben el permiso de plazo (S3), como en ARM.
    ("sonido", elf!("sonido"), &[("plazo", PLAZO)], &["sonido"]),
    ("virtio-sonido", elf!("virtio-sonido"), &[("dma-vsonido", PERMISO_DMA), ("virtio-sonido", REGISTROS), ("vsonido-cfg", REGISTROS), ("plazo", PLAZO), ("virtio-sonido", INTERRUPCION | OPCIONAL)], &["sonido"]),
    // X5c: el sonido de un PC (Intel HDA). Si hay los dos, suena el primero que se presenta al servicio.
    ("hda", elf!("hda"), &[("dma-hda", PERMISO_DMA), ("hda", REGISTROS), ("plazo", PLAZO), ("hda", INTERRUPCION | OPCIONAL)], &["sonido"]),
    ("tono-a", elf!("tono-a"), &[], &["sonido"]),
    ("tono-b", elf!("tono-b"), &[], &["sonido"]),
    // X5c: la red, como en `virt` (S13): el servicio con la pila (nombra "red" y "red-estado" antes que nadie) y el
    // controlador de virtio-net, con su MSI-X. Y sus pruebas: el eco del PC y un glotón de conexiones.
    ("red", elf!("red"), &[], &["red", "red-estado"]),
    ("virtio-red", elf!("virtio-red"), &[("dma-vred", PERMISO_DMA), ("virtio-red", REGISTROS), ("vred-cfg", REGISTROS), ("virtio-red", INTERRUPCION | OPCIONAL)], &["red"]),
    // X5c: la red con cable de Intel: el I219 del PCH (pegaso) y el 82574L (el `e1000e` de QEMU). Como virtio-red.
    ("intel-red", elf!("intel-red"), &[("dma-ired", PERMISO_DMA), ("intel-red", REGISTROS), ("ired-cfg", REGISTROS), ("intel-red", INTERRUPCION | OPCIONAL)], &["red"]),
    // X5c: la red con cable de Realtek (RTL8125B, la de shiva), con el firmware de su PHY (`FIRMWARE_RTL8125`).
    ("rtl-red", elf!("rtl-red"), &[("dma-rtl", PERMISO_DMA), ("rtl-red", REGISTROS), ("rtl-cfg", REGISTROS), ("rtl8125b-fw", DATOS), ("rtl-red", INTERRUPCION | OPCIONAL)], &["red"]),
    ("red-gloton", elf!("red-gloton"), &[], &["red"]),
    ("red-prueba", elf!("red-prueba"), &[], &["red"]),
    // Dos clientes de Wayland de prueba (W7) y una aplicación con el kit (K7), como en ARM.
    ("ventana-a", elf!("ventana-a"), &[], &["ventanas"]),
    ("ventana-b", elf!("ventana-b"), &[], &["ventanas"]),
    ("ajustes", elf!("ajustes"), &[], &["ventanas"]),
    ("hola", elf!("hola"), &[], &[]),
];

const REGISTROS: u32 = 1;
const INTERRUPCION: u32 = 2;
const PERMISO_DMA: u32 = 3;
const REGISTRO: u32 = 4;
const FIRMWARE: u32 = 5;
/// Un objeto de memoria con datos, de solo lectura (las medidas de la pantalla).
const DATOS: u32 = 6;
/// El permiso de pedir la clase de plazo (S3), como en `inicio.rs`.
const PLAZO: u32 = 7;
/// Puertos de E/S (X6): los de COM1, para la consola.
const PUERTOS: u32 = 12;
/// COM1: la UART 16550 de un PC, ocho puertos desde 0x3F8.
const COM1: (u32, u32) = (0x3f8, 8);
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
/// (especificación de xHCI); NVMe, 0x010802 (especificación de NVMe); AHCI, 0x010601, con sus registros (ABAR) en la
/// BAR 5 (Serial ATA AHCI 1.3.1, 2.1.11); Intel HDA, 0x040300, con los suyos en la BAR 0 (HDA 1.0a, 3.3). El sonido de virtio: virtio 1.2, 4.1.2,
/// fabricante 0x1af4 y aparato 0x1040 más el tipo, 25; en QEMU, todos sus registros en la BAR 4. La red de virtio, igual
/// (X5c). La red de Intel (X5c, `docs/spec/hw/i219.md`, 2.1 y 14.1): varios aparatos de Intel (0x8086), los registros
/// en la BAR 0. La de Realtek (X5c), 0x10ec:0x8125, en la BAR 2.
enum Quien {
    Clase(u32),
    Id(u16, u16),
    Ids(u16, &'static [u16]),
}
/// Los de `intel-red`: el I219 de la familia "Cannon Point" (la de pegaso, 0x15BC, y sus hermanos) y el 82574L (0x10D3,
/// el `e1000e` de QEMU) con el 82574LA (0x10F6). Las demás generaciones del I219 tienen ajustes propios y aún no van.
const INTEL_RED: &[u16] = &[0x15BB, 0x15BC, 0x15BD, 0x15BE, 0x15DF, 0x15E0, 0x15E1, 0x15E2, 0x0D4C, 0x0D4D, 0x0D4E, 0x0D4F, 0x10D3, 0x10F6];
const APARATOS: &[(&str, Quien, usize, Option<&str>, &str)] = &[
    ("virtio-sonido", Quien::Id(0x1af4, 0x1059), 4, Some("vsonido-cfg"), "dma-vsonido"),
    ("xhci", Quien::Clase(0x0C_0330), 0, None, "dma-xhci"),
    ("nvme", Quien::Clase(0x01_0802), 0, None, "dma-nvme"),
    ("ahci", Quien::Clase(0x01_0601), 5, None, "dma-ahci"),
    ("hda", Quien::Clase(0x04_0300), 0, None, "dma-hda"),
    // La red de virtio: fabricante 0x1af4 y aparato 0x1040 más el tipo, 1 (virtio 1.2, 4.1.2); solo el moderno.
    ("virtio-red", Quien::Id(0x1af4, 0x1041), 4, Some("vred-cfg"), "dma-vred"),
    ("intel-red", Quien::Ids(0x8086, INTEL_RED), 0, Some("ired-cfg"), "dma-ired"),
    // La Realtek RTL8125 (`docs/spec/hw/rtl8125.md`, 2.1): los registros en la BAR 2 (la 4 es la tabla de MSI-X).
    ("rtl-red", Quien::Id(0x10ec, 0x8125), 2, Some("rtl-cfg"), "dma-rtl"),
];

/// El firmware del PHY de la RTL8125B (`rtl8125b-2.fw`, `docs/spec/hw/rtl8125.md` 6.3), si el sistema donde se compila
/// lo tiene: el Makefile lo deja en `target/firmware/` y `build.rs` lo copia (vacío si no está). No va en git: es de
/// Realtek, con su licencia (`LICENSE.r8169` de linux-firmware), que tiene que acompañar a una imagen que se reparta.
static FIRMWARE_RTL8125: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rtl8125b-2.fw"));

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
    // Con el remapeo de VT-d, en formato remapeable y con su entrada en la tabla (`iommu::dar_msi`).
    let irq = interrupciones::msi().and_then(|(intid, direccion, dato)| crate::iommu::dar_msi(f, direccion, dato).map(|como| (intid, como)));
    match irq {
        Some((i, como)) => kprintln!("{}: en el PCIe {}:{}.{}, registros en {:#x} ({} KB), {} en la {}", nombre, f.bus, f.aparato, f.funcion, base, tamano / 1024, como, i),
        None => kprintln!("{}: en el PCIe {}:{}.{}, registros en {:#x} ({} KB), sin MSI", nombre, f.bus, f.aparato, f.funcion, base, tamano / 1024),
    }
    Some((base, paginas, irq.map(|(i, _)| i)))
}

/// La pantalla del GOP que deja el cargador en la `InfoArranque`, si la hay: (memoria, objeto con sus medidas), como
/// el nodo `simple-framebuffer` en ARM (`inicio.rs`, U4). Las medidas: ancho, alto, bytes por línea, formato (0
/// a8r8g8b8, 1 a8b8g8r8) y desplazamiento dentro de la página. El formato del GOP (UEFI 2.10, 12.9) es al revés: 0
/// es R, G, B en memoria (a8b8g8r8) y 1, B, G, R (a8r8g8b8); el cargador solo pasa estos dos, de 32 bits.
fn pantalla_del_gop() -> Option<(Objeto, Objeto)> {
    let i = crate::base::arranque::info()?;
    if i.pantalla_base == 0 || i.pantalla_tamano == 0 || i.pantalla_formato > 1 {
        return None;
    }
    let (fisica, tamano) = (i.pantalla_base, i.pantalla_tamano);
    let paso = i.pantalla_por_linea.checked_mul(4)?;
    if (paso as u64).checked_mul(i.pantalla_alto as u64)? > tamano || i.pantalla_ancho > i.pantalla_por_linea {
        return None;
    }
    let formato = 1 - i.pantalla_formato;
    let pagina = fisica & !(memoria::PAGINA - 1);
    let fin = fisica.checked_add(tamano)?.div_ceil(memoria::PAGINA) * memoria::PAGINA;
    kprintln!("pantalla del firmware: {}x{} en {:#x} ({} KB)", i.pantalla_ancho, i.pantalla_alto, fisica, tamano / 1024);
    let info = memoria::Memoria::nueva(memoria::PAGINA as usize, &crate::cupo::Cupo::raiz())?;
    let mut datos = [0u8; 20];
    for (k, v) in [i.pantalla_ancho, i.pantalla_alto, paso, formato, (fisica - pagina) as u32].iter().enumerate() {
        datos[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
    }
    info.escribir(0, &datos);
    Some((Objeto::Firmware { fisica: pagina, tamano: fin - pagina }, Objeto::Memoria(alloc::sync::Arc::new(info))))
}

/// Arma las llaves de arranque y lanza `inicio`. Falso si no se pudo.
pub fn lanzar(_a: &crate::base::acpi::Acpi, funciones: &[Funcion]) -> bool {
    let mut llaves: Vec<Llave> = Vec::new();
    let mut manifiesto: Vec<([u8; 16], u32, u32)> = Vec::new();
    // La llave 1 es el paquete; las de los aparatos empiezan en la 2.
    let mut apuntar = |llaves: &mut Vec<Llave>, nombre: &str, tipo: u32, l: Llave| {
        llaves.push(l);
        manifiesto.push((nombre16(nombre), tipo, llaves.len() as u32 + 1));
    };
    // La consola (X6): el registro del sistema y los puertos de COM1, como el registro y el PL011 en ARM.
    apuntar(&mut llaves, "registro", REGISTRO, Llave::nueva(Objeto::Registro, DERECHOS));
    apuntar(&mut llaves, "serie", PUERTOS, Llave::nueva(Objeto::Puertos { primero: COM1.0, cuantos: COM1.1 }, DERECHOS));
    // El permiso de plazo (S3): para el sonido y su controlador.
    apuntar(&mut llaves, "plazo", PLAZO, Llave::nueva(Objeto::PermisoPlazo, DERECHOS));
    // La pantalla del GOP (X6): su memoria, como memoria del firmware (fuera de la RAM), y sus medidas.
    if let Some((pantalla, info)) = pantalla_del_gop() {
        apuntar(&mut llaves, "pantalla", FIRMWARE, Llave::nueva(pantalla, DERECHOS));
        apuntar(&mut llaves, "pantalla-info", DATOS, Llave::nueva(info, LEER | crate::objetos::TRANSFERIR));
    }
    let mut con_rtl = false;
    for (nombre, quien, bar, cfg, dma) in APARATOS {
        let Some(f) = funciones.iter().find(|f| match quien {
            Quien::Clase(c) => f.clase == *c,
            Quien::Id(fab, ap) => f.id == (*ap as u32) << 16 | *fab as u32,
            Quien::Ids(fab, aps) => f.id & 0xffff == *fab as u32 && aps.contains(&((f.id >> 16) as u16)),
        }) else {
            continue;
        };
        // X11: el aparato es de este servicio; ya no se puede pasar a una máquina virtual.
        if !crate::pasar::reclamar(f) {
            continue;
        }
        // Con IOMMU (X5b), su dominio: la memoria DMA que pida con su permiso es lo único que alcanza.
        let permiso = match crate::iommu::dominio_para(f) {
            Ok(Some(d)) => {
                kprintln!("{}: DMA por la IOMMU, en su dominio ({})", nombre, d.id());
                memoria::PermisoDma::con_dominio(memoria::Zona::Dma1G, d)
            }
            Ok(None) => {
                if crate::iommu::activa() {
                    kprintln!("{}: ninguna IOMMU lo lleva; su DMA no se limita", nombre);
                }
                memoria::PermisoDma::nuevo(memoria::Zona::Dma1G, 0)
            }
            Err(()) => {
                kprintln!("{}: la IOMMU no le puede dar dominio; no se usa", nombre);
                continue;
            }
        };
        let Some((fisica, tamano, irq)) = preparar(f, *bar, nombre) else { continue };
        apuntar(&mut llaves, nombre, REGISTROS, Llave::nueva(Objeto::Registros { fisica, tamano }, DERECHOS));
        con_rtl |= *nombre == "rtl-red";
        if let Some(c) = cfg {
            apuntar(&mut llaves, c, REGISTROS, Llave::nueva(Objeto::Registros { fisica: f.fisica_cfg, tamano: memoria::PAGINA }, DERECHOS));
        }
        if let Some(irq) = irq.and_then(interrupciones::Interrupcion::nueva) {
            apuntar(&mut llaves, nombre, INTERRUPCION, Llave::nueva(Objeto::Interrupcion(irq), DERECHOS));
        }
        let permiso = Objeto::PermisoDma(alloc::sync::Arc::new(permiso));
        apuntar(&mut llaves, dma, PERMISO_DMA, Llave::nueva(permiso, DERECHOS));
    }
    // El firmware de la RTL8125, si está su tarjeta: su tamaño (u32) y el fichero; tamaño 0 si no se tiene. Siempre que
    // esté la tarjeta, para que las llaves de `rtl-red` no cambien de número.
    if con_rtl {
        if let Some(m) = memoria::Memoria::nueva(4 + FIRMWARE_RTL8125.len(), &crate::cupo::Cupo::raiz()) {
            m.escribir(0, &(FIRMWARE_RTL8125.len() as u32).to_le_bytes());
            m.escribir(4, FIRMWARE_RTL8125);
            apuntar(&mut llaves, "rtl8125b-fw", DATOS, Llave::nueva(Objeto::Memoria(alloc::sync::Arc::new(m)), LEER | crate::objetos::TRANSFERIR));
        }
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
    // X5c: sin NVMe, el AHCI sirve el canal "disco" (y los de "sata" van a él).
    let hay_nvme = manifiesto.iter().any(|(n, t, _)| *n == nombre16("nvme") && *t == REGISTROS);
    let canal = |c: &'static str| if c == "sata" && !hay_nvme { "disco" } else { c };
    let mut canales: Vec<&str> = Vec::new();
    for (nombre, elf, aparatos, suyos) in lanzables.iter() {
        let mut suyos_aparatos = [0xFFu8; 8];
        for (k, (aparato, tipo)) in aparatos.iter().enumerate().take(8) {
            if let Some(i) = manifiesto.iter().position(|(n, t, _)| *n == nombre16(aparato) && *t == tipo & !OPCIONAL) {
                suyos_aparatos[k] = i as u8;
            }
        }
        let mut de_canales = 0u32;
        for c in suyos.iter().map(|c| canal(c)) {
            let i = canales.iter().position(|x| *x == c).unwrap_or_else(|| {
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
