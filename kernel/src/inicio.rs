//! El programa de inicio (C47): lo primero que corre en EL0 en la imagen normal.
//!
//! El kernel lo lanza en el grupo raíz con las llaves de arranque, las únicas que da él:
//! - la 1, un objeto de memoria con el paquete de arranque (solo lectura);
//! - después, una por aparato: el permiso DMA y, para cada controlador, sus registros y su interrupción.
//!
//! El paquete dice qué es cada llave y trae los ELF de los servicios, para que `inicio` los lance. Formato
//! (little-endian; lo lee `usuario/src/bin/inicio.rs`):
//! - u32 cuántos aparatos; por cada uno, 24 bytes: nombre (16, relleno con 0), tipo (u32: 1 registros,
//!   2 interrupción, 3 permiso DMA, 4 registro, 5 memoria del firmware) y número de llave (u32);
//! - u32 cuántos programas; por cada uno, 32 bytes: tamaño (u32), qué canales (u32: el bit i es el canal i; C50),
//!   qué aparatos le pasa `inicio` (8 bytes: el número de cada uno en la lista de arriba, en el orden en que los pide
//!   el programa, y 0xFF en los huecos) y nombre (16). Sus llaves serán la 1, la 2...: primero los aparatos, en ese
//!   orden, y después los canales. `inicio` le da una
//!   copia de cada llave de aparato (se queda la suya) y crea cada canal la primera vez que se nombra: el primer
//!   programa que lo nombra recibe un extremo y el segundo, el otro;
//! - los ELF seguidos, cada uno desde un múltiplo de 8.

use crate::dtb::Dtb;
use crate::objetos::{Llave, Objeto, LEER};
use crate::{interrupciones, kprintln, memoria};
use alloc::vec::Vec;

static INICIO: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/inicio");

/// Los servicios que lanza `inicio`, en orden, con los aparatos (por nombre y tipo) y los canales (por nombre) que le
/// pasa a cada uno. Lo que ve cada servicio de los demás está en `docs/referencia/servicios.md`.
static PROGRAMAS: &[(&str, &[u8], &[(&str, u32)], &[&str])] = &[
    ("consola", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/consola"), &[("registro", REGISTRO), ("serie", REGISTROS)], &[]),
    ("buzon", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/buzon"), &[("dma-buzon", PERMISO_DMA), ("buzon", REGISTROS), ("vc", FIRMWARE)], &["buzon"]),
    // El compositor nombra "ventanas" antes que los controladores de USB y que los clientes de Wayland: el primero que
    // nombra un canal es quien lee (el servidor), y los demás le escriben con copias marcadas (U6, W7). Un proceso solo
    // espera en un canal, así que por él le llega todo.
    #[cfg(not(feature = "virt"))]
    ("compositor", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/compositor"), &[], &["buzon", "ventanas"]),
    // En QEMU `virt` (U4) no hay buzón: la pantalla es la del firmware de UEFI, que llega en el árbol.
    #[cfg(feature = "virt")]
    ("compositor", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/compositor"), &[("pantalla", FIRMWARE), ("pantalla-info", DATOS)], &["ventanas"]),
    ("usb", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/usb"), &[("dma", PERMISO_DMA), ("usb", REGISTROS), ("usb", INTERRUPCION)], &["ventanas"]),
    // El USB por xHCI (U6): el de los portátiles y el de QEMU `virt` (y, en la Pi 4, el VL805, por el PCIe).
    // Su interrupción (INTx) es opcional: con ACPI no se sabe cuál es (va en el AML) y mira los eventos con un
    // temporizador.
    ("xhci", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/xhci"), &[("dma-xhci", PERMISO_DMA), ("xhci", REGISTROS), ("xhci", INTERRUPCION | OPCIONAL)], &["ventanas"]),
    // El disco NVMe (U6d), que sirve el disco por el canal "disco" (U6f).
    ("nvme", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/nvme"), &[("dma-nvme", PERMISO_DMA), ("nvme", REGISTROS), ("nvme", INTERRUPCION | OPCIONAL)], &["disco"]),
    // Una prueba del servicio de disco (U6f), solo en `virt`: la Pi aún no tiene servicio de disco.
    #[cfg(feature = "virt")]
    ("disco", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/disco"), &[], &["disco"]),
    // El servicio de archivos (FS1, S5): sirve el RedoxFS del mismo disco por el canal "archivos" (FS3), con un
    // cliente de prueba. Solo en `virt`, como la prueba de arriba.
    #[cfg(feature = "virt")]
    ("archivos", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/archivos"), &[], &["disco", "archivos"]),
    #[cfg(feature = "virt")]
    ("ficheros", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/ficheros"), &[], &["archivos"]),
    ("gpu", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/gpu"), &[("dma-gpu", PERMISO_DMA), ("v3d", REGISTROS), ("v3d-puente", REGISTROS), ("pm", REGISTROS), ("cprman", REGISTROS)], &["buzon"]),
    // Dos clientes de Wayland de prueba (W7), cada uno con su ventana.
    ("ventana-a", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/ventana-a"), &[], &["ventanas"]),
    ("ventana-b", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/ventana-b"), &[], &["ventanas"]),
    // Una aplicación con el kit de interfaz (K7): Material 3, con el ratón.
    ("ajustes", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/ajustes"), &[], &["ventanas"]),
    // El sonido (S19): el servicio, que nombra "sonido" antes que nadie (A4), y el controlador de virtio-sound (A3),
    // si lo hay, que espera su interrupción (o, sin ella, mira sus colas con un temporizador).
    ("sonido", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/sonido"), &[], &["sonido"]),
    ("virtio-sonido", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/virtio-sonido"), &[("dma-vsonido", PERMISO_DMA), ("virtio-sonido", REGISTROS), ("vsonido-cfg", REGISTROS), ("virtio-sonido", INTERRUPCION | OPCIONAL)], &["sonido"]),
    // Dos programas que suenan a la vez por el servicio (A4), solo en `virt`.
    #[cfg(feature = "virt")]
    ("tono-a", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/tono-a"), &[], &["sonido"]),
    #[cfg(feature = "virt")]
    ("tono-b", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/tono-b"), &[], &["sonido"]),
    // Cuatro programas que esperan medio segundo y giran sin parar 6 s, detrás de los tonos: los cortes del sonido con los núcleos ocupados (A6).
    #[cfg(all(feature = "virt", feature = "carga"))]
    ("carga-1", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/carga"), &[], &[]),
    #[cfg(all(feature = "virt", feature = "carga"))]
    ("carga-2", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/carga"), &[], &[]),
    #[cfg(all(feature = "virt", feature = "carga"))]
    ("carga-3", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/carga"), &[], &[]),
    #[cfg(all(feature = "virt", feature = "carga"))]
    ("carga-4", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/carga"), &[], &[]),
    ("hola", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/hola"), &[], &[]),
];

const REGISTROS: u32 = 1;
const INTERRUPCION: u32 = 2;
const PERMISO_DMA: u32 = 3;
const REGISTRO: u32 = 4;
const FIRMWARE: u32 = 5;
/// Un objeto de memoria con datos, de solo lectura (U4: las medidas de la pantalla).
const DATOS: u32 = 6;
/// Marca de un aparato que el servicio puede no tener: si falta, se lanza igual, sin esa llave (y las siguientes
/// corren un puesto).
const OPCIONAL: u32 = 0x100;
/// Las llaves de arranque llevan todos los derechos: `inicio` las reparte (transferir) a los servicios.
const DERECHOS: u32 = crate::objetos::TODOS;

fn nombre16(n: &str) -> [u8; 16] {
    let mut b = [0u8; 16];
    let l = n.len().min(16);
    b[..l].copy_from_slice(&n.as_bytes()[..l]);
    b
}

/// Los aparatos que se dan a `inicio`: nombre, compatible del árbol y si se da su interrupción.
const APARATOS: &[(&str, &str, bool)] = &[
    ("serie", "arm,pl011", true),
    ("sd", "brcm,bcm2711-emmc2", true),
    ("usb", "brcm,bcm2708-usb", true),
    ("buzon", "brcm,bcm2835-mbox", false),
];

/// Cómo se busca un aparato del PCIe: por su clase (clase, subclase e interfaz) o por fabricante y número de aparato.
#[derive(Clone, Copy)]
enum Pci {
    Clase(u32),
    Id(u16, u16),
}

/// El primer aparato que case con `quien` en el PCIe genérico de ECAM ("pci-host-ecam-generic", el de QEMU `virt` y
/// las máquinas con UEFI): (física y tamaño de la BAR del registro `bar` de su configuración, 0x10 para la 0, en
/// páginas enteras; su interrupción; y la física de su espacio de configuración, 4 KB). Mira el bus 0. Enciende la decodificación de memoria y el bus master, y
/// mide la BAR escribiendo unos (con la decodificación apagada mientras). Solo si el ECAM y la BAR caen en la ventana
/// de aparatos de la placa. Escrito con la especificación de PCI (espacio de configuración, cabecera de tipo 0). Las
/// clases: xHCI, 0x0C0330 (especificación de xHCI); NVMe, 0x010802 (especificación de NVMe).
fn aparato_pci(d: &Dtb, quien: Pci, bar: u64, nombre: &str) -> Option<(u64, u64, Option<u32>, u64)> {
    let n = d.compatible("pci-host-ecam-generic")?;
    let (ecam, _) = d.reg_cpu(&n, 0)?;
    if !crate::placa::es_periferico(ecam) {
        return None;
    }
    let r = |dispositivo: u64, off: u64| crate::mmio::RegLe32::new(crate::mmu::virt((ecam + (dispositivo << 15) + off) as usize));
    for dev in 0..32 {
        if r(dev, 0).leer() == 0xffff_ffff {
            continue;
        }
        let casa = match quien {
            Pci::Clase(clase) => r(dev, 8).leer() >> 8 == clase,
            Pci::Id(fabricante, aparato) => r(dev, 0).leer() == (aparato as u32) << 16 | fabricante as u32,
        };
        if !casa {
            continue;
        }
        let (bar0, bar1) = (r(dev, bar), r(dev, bar + 4));
        let bajo = bar0.leer();
        let es64 = bajo & 0x6 == 0x4;
        let alto = if es64 { bar1.leer() } else { 0 };
        let base = ((alto as u64) << 32) | (bajo & !0xf) as u64;
        // Medir: decodificación apagada, unos, leer, y volver a poner.
        let mando = r(dev, 4);
        let antes = mando.leer() & 0xffff;
        mando.escribir(antes & !0x7);
        bar0.escribir(0xffff_ffff);
        let mascara = bar0.leer() & !0xf;
        bar0.escribir(bajo);
        let tamano = (!mascara).wrapping_add(1) as u64;
        // Memoria y bus master encendidos (los bits 1 y 2 del registro de mando; el de estado no se toca), y la INTx
        // permitida (el bit 10 la apaga).
        mando.escribir((antes & !0x400) | 0x6);
        let paginas = tamano.max(memoria::PAGINA).div_ceil(memoria::PAGINA) * memoria::PAGINA;
        if base == 0 || !crate::placa::es_periferico(base) || !crate::placa::es_periferico(base + paginas - 1) {
            kprintln!("{}: BAR en {:#x} fuera de la ventana de aparatos", nombre, base);
            return None;
        }
        // Su interrupción: una MSI-X por el GICv2m si hay los dos (U6g); si no, la INTx de su patilla (1 a 4, INTA a
        // INTD, en el byte 0x3D; 0, sin INTx), por el `interrupt-map` (U6e).
        // La MSI-X solo con la tabla en la BAR 0 (la de `msix`).
        let msi = if bar == 0x10 { msix(d, &|off| r(dev, off), base, id_msi(d, &n, (dev as u32) << 3)) } else { None };
        let patilla = (r(dev, 0x3C).leer() >> 8) & 0xff;
        let irq = msi.or_else(|| if patilla != 0 { intx(d, &n, dev as u32, patilla) } else { None });
        match irq {
            Some(i) => kprintln!("{}: en el PCIe 0:{}.0, registros en {:#x} ({} KB), {} en la {}", nombre, dev, base, tamano / 1024, if msi.is_some() { "MSI-X" } else { "INTx" }, i),
            None => kprintln!("{}: en el PCIe 0:{}.0, registros en {:#x} ({} KB)", nombre, dev, base, tamano / 1024),
        }
        return Some((base, paginas, irq, ecam + (dev << 15)));
    }
    None
}

/// El número de aparato que ve el controlador de MSI (el DeviceID del ITS) para un RID del PCIe, por el `msi-map` del
/// nodo del PCIe (U6j; binding "pci-msi" de devicetree: entradas de RID base, phandle del controlador, número base y
/// cuántos). Sin `msi-map`, el mismo RID. Con ACPI, el cargador lo escribe desde el IORT.
fn id_msi(d: &Dtb, pcie: &crate::dtb::Nodo, rid: u32) -> u32 {
    let Some((mapa, largo)) = d.propiedad(pcie, "msi-map") else { return rid };
    for i in 0..largo / 16 {
        let (base, _, salida, cuantos) = (d.celda(mapa, 4 * i), d.celda(mapa, 4 * i + 1), d.celda(mapa, 4 * i + 2), d.celda(mapa, 4 * i + 3));
        if rid >= base && rid - base < cuantos {
            return salida + (rid - base);
        }
    }
    rid
}

/// Si el ITS está encendido (U6h): las MSI van por él.
static ITS_LISTO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// La siguiente SPI libre del GICv2m (U6g), 0 si aún no se ha leído su MSI_TYPER.
static MSI_SIGUIENTE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Una MSI-X para un aparato del PCIe por el ITS del GICv3 (U6h; `rid` es su número para el ITS, U6j) o por el marco GICv2m (el de QEMU `virt` con GICv2 y el de la MADT de ACPI; spec.
/// del GICv2m en la SBSA, apéndice E): el aparato escribe el número de una SPI en MSI_SETSPI_NS (0x40) del marco, y el
/// GIC la levanta, por flanco. Da al aparato la primera entrada de su tabla de MSI-X (spec. de PCI 3.0, 6.8.2: la
/// capacidad 0x11, con el control en los bits 31:16 de su primera palabra y la tabla en una BAR, aquí solo la 0) y
/// enciende la MSI-X, que apaga la INTx. `cfg` lee y escribe la configuración del aparato; `bar0`, su BAR 0 física.
fn msix(d: &Dtb, cfg: &dyn Fn(u64) -> crate::mmio::RegLe32, bar0: u64, rid: u32) -> Option<u32> {
    use core::sync::atomic::Ordering;
    // Con ITS (U6h) va por él; si no, por un marco GICv2m.
    let its = ITS_LISTO.load(Ordering::Relaxed);
    let marco = if its { None } else { Some(d.compatible("arm,gic-v2m-frame")?) };
    let (fisica, primera, cuantas) = match marco {
        Some(m) => {
            let (fisica, _) = d.reg_cpu(&m, 0)?;
            if !crate::placa::es_periferico(fisica) {
                return None;
            }
            // MSI_TYPER (0x08): la primera SPI en los bits 25:16 y cuántas en los 9:0.
            let typer = crate::mmio::RegLe32::new(crate::mmu::virt(fisica as usize + 0x08)).leer();
            (fisica, (typer >> 16) & 0x3ff, typer & 0x3ff)
        }
        None => (0, 0, 0),
    };
    if !its {
        let _ = MSI_SIGUIENTE.compare_exchange(0, primera, Ordering::Relaxed, Ordering::Relaxed);
    }
    // La lista de capacidades (bit 4 del estado), desde el puntero de 0x34.
    if (cfg(4).leer() >> 16) & (1 << 4) == 0 {
        return None;
    }
    let mut p = (cfg(0x34).leer() & 0xfc) as u64;
    let mut vueltas = 0;
    while p != 0 && vueltas < 48 {
        let palabra = cfg(p).leer();
        if palabra & 0xff == 0x11 {
            let tabla = cfg(p + 4).leer();
            if tabla & 7 != 0 {
                return None; // la tabla en otra BAR: aún no
            }
            let (destino, dato, intid) = if its {
                crate::its::msi(rid)?
            } else {
                let spi = MSI_SIGUIENTE.fetch_add(1, Ordering::Relaxed);
                if spi >= primera + cuantas {
                    return None;
                }
                crate::gic::por_flanco(spi);
                (fisica + 0x40, spi, spi)
            };
            let entrada = crate::mmu::virt((bar0 + (tabla & !7) as u64) as usize);
            crate::mmio::RegLe32::new(entrada).escribir(destino as u32);
            crate::mmio::RegLe32::new(entrada + 4).escribir((destino >> 32) as u32);
            crate::mmio::RegLe32::new(entrada + 8).escribir(dato);
            crate::mmio::RegLe32::new(entrada + 12).escribir(0); // sin máscara
            // Control: encendida (bit 15) y sin la máscara de función (bit 14).
            cfg(p).escribir((palabra & 0x3fff_ffff) | (1 << 31));
            return Some(intid);
        }
        p = ((palabra >> 8) & 0xfc) as u64;
        vueltas += 1;
    }
    None
}

/// La interrupción del GIC de la patilla `patilla` del aparato `dev` del bus 0, por el `interrupt-map` del nodo del
/// PCIe (especificación de devicetree, 2.4.3; "PCI Bus Binding to Open Firmware"): cada entrada es la dirección del
/// hijo (3 celdas: en la primera, el aparato en los bits 15:11), su patilla (1 celda), el phandle del padre, la
/// dirección del padre (sus #address-cells) y su interrupción (sus #interrupt-cells: tipo, número, flags), y se compara
/// con `interrupt-map-mask` aplicada. Solo vale con el GIC como padre.
fn intx(d: &Dtb, pcie: &crate::dtb::Nodo, dev: u32, patilla: u32) -> Option<u32> {
    let (mapa, largo) = d.propiedad(pcie, "interrupt-map")?;
    let (mascara, _) = d.propiedad(pcie, "interrupt-map-mask")?;
    let (m0, m3) = (d.celda(mascara, 0), d.celda(mascara, 3));
    let (hijo_dir, hijo_pat) = (dev << 11, patilla);
    let celdas = |phandle: u32| -> Option<(usize, usize)> {
        let mut r = None;
        d.cada_nodo(|n| {
            if r.is_none() && d.propiedad(n, "phandle").is_some_and(|(p, _)| d.celda(p, 0) == phandle) {
                let c = |nombre: &str, def: u32| d.propiedad(n, nombre).map_or(def, |(p, _)| d.celda(p, 0)) as usize;
                r = Some((c("#address-cells", 0), c("#interrupt-cells", 3)));
            }
        });
        r
    };
    let mut i = 0;
    while (i + 5) * 4 <= largo {
        let (dir, pat, phandle) = (d.celda(mapa, i), d.celda(mapa, i + 3), d.celda(mapa, i + 4));
        let (pdir, pirq) = celdas(phandle)?;
        if pirq < 2 || (i + 5 + pdir + pirq) * 4 > largo {
            return None;
        }
        if dir & m0 == hijo_dir & m0 && pat & m3 == hijo_pat & m3 {
            let (tipo, num) = (d.celda(mapa, i + 5 + pdir), d.celda(mapa, i + 6 + pdir));
            return Some(if tipo == 0 { 32 + num } else { 16 + num });
        }
        i += 5 + pdir + pirq;
    }
    None
}

/// La pantalla de un nodo `simple-framebuffer` con `status = "okay"`, si lo hay: (memoria, objeto con sus medidas).
fn pantalla_del_firmware(d: &Dtb) -> Option<(Objeto, Objeto)> {
    let n = d.compatible("simple-framebuffer")?;
    let (fisica, tamano) = d.reg_cpu(&n, 0)?;
    let celda = |nombre: &str| d.propiedad(&n, nombre).map(|(datos, _)| d.celda(datos, 0));
    let (ancho, alto, paso) = (celda("width")?, celda("height")?, celda("stride")?);
    let formato = match d.propiedad(&n, "format") {
        Some((datos, _)) if d.u8(datos + 1) == b'8' && d.u8(datos + 2) == b'b' => 1, // a8b8g8r8
        _ => 0,                                                                     // a8r8g8b8
    };
    let pagina = fisica & !(memoria::PAGINA - 1);
    let fin = (fisica + tamano).div_ceil(memoria::PAGINA) * memoria::PAGINA;
    kprintln!("pantalla del firmware: {}x{} en {:#x} ({} KB)", ancho, alto, fisica, tamano / 1024);
    let info = memoria::Memoria::nueva(memoria::PAGINA as usize, &crate::cupo::Cupo::raiz())?;
    let mut datos = [0u8; 20];
    for (i, v) in [ancho, alto, paso, formato, (fisica - pagina) as u32].iter().enumerate() {
        datos[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    info.escribir(0, &datos);
    Some((Objeto::Firmware { fisica: pagina, tamano: fin - pagina }, Objeto::Memoria(alloc::sync::Arc::new(info))))
}

/// Arma las llaves de arranque y lanza `inicio`. Falso si no se pudo.
pub fn lanzar() -> bool {
    let Some(d) = Dtb::desde() else { return false };
    let mut llaves: Vec<Llave> = Vec::new();
    let mut manifiesto: Vec<([u8; 16], u32, u32)> = Vec::new();
    // La llave 1 es el paquete; las de los aparatos empiezan en la 2.
    let mut apuntar = |llaves: &mut Vec<Llave>, nombre: &str, tipo: u32, l: Llave| {
        llaves.push(l);
        manifiesto.push((nombre16(nombre), tipo, llaves.len() as u32 + 1));
    };
    if let Some((bus, cpu, _)) = d.por_nombre("emmc2bus").and_then(|n| d.ventana_dma(&n, 0)) {
        // Uno por aparato, cada uno con su cuarentena (auditoría de la fase 4, M5): el del USB y el del buzón.
        for nombre in ["dma", "dma-buzon"] {
            let permiso = Objeto::PermisoDma(alloc::sync::Arc::new(memoria::PermisoDma::nuevo(memoria::Zona::Dma1G, bus.wrapping_sub(cpu))));
            apuntar(&mut llaves, nombre, PERMISO_DMA, Llave::nueva(permiso, DERECHOS));
        }
    }
    apuntar(&mut llaves, "registro", REGISTRO, Llave::nueva(Objeto::Registro, DERECHOS));
    if let Some((fisica, tamano)) = d.memoria_vc() {
        apuntar(&mut llaves, "vc", FIRMWARE, Llave::nueva(Objeto::Firmware { fisica, tamano }, DERECHOS));
    }
    for &(nombre, compatible, con_irq) in APARATOS {
        let Some(n) = d.compatible(compatible) else { continue };
        let Some((fisica, tamano)) = d.reg_cpu(&n, 0) else { continue };
        // La llave cubre páginas enteras: el buzón, por ejemplo, empieza a mitad de una (0x880).
        let pagina = fisica & !(memoria::PAGINA - 1);
        let tamano = (fisica + tamano - pagina).div_ceil(memoria::PAGINA) * memoria::PAGINA;
        let fisica = pagina;
        apuntar(&mut llaves, nombre, REGISTROS, Llave::nueva(Objeto::Registros { fisica, tamano }, DERECHOS));
        if con_irq {
            if let Some((datos, _)) = d.propiedad(&n, "interrupts") {
                // interrupts = <tipo número flags>; tipo 0 es SPI, que en el GIC empiezan en la 32.
                let intid = if d.celda(datos, 0) == 0 { 32 + d.celda(datos, 1) } else { 16 + d.celda(datos, 1) };
                if let Some(irq) = interrupciones::Interrupcion::nueva(intid) {
                    apuntar(&mut llaves, nombre, INTERRUPCION, Llave::nueva(Objeto::Interrupcion(irq), DERECHOS));
                }
            }
        }
    }
    // La pantalla que dejó el firmware (U4): un nodo `simple-framebuffer` del árbol, que pone el cargador UEFI. Su
    // memoria, como memoria del firmware (está fuera de la RAM o en una zona reservada), y sus medidas en un objeto de
    // memoria: ancho, alto, bytes por línea, formato (0 a8r8g8b8, 1 a8b8g8r8) y desplazamiento dentro de la página.
    if let Some((pantalla, info)) = pantalla_del_firmware(&d) {
        apuntar(&mut llaves, "pantalla", FIRMWARE, Llave::nueva(pantalla, DERECHOS));
        apuntar(&mut llaves, "pantalla-info", DATOS, Llave::nueva(info, LEER | crate::objetos::TRANSFERIR));
    }
    // La V3D (C50): solo si su puente contesta. QEMU no la emula, y leer su ventana daría un fallo de bus: se sondea.
    // Sus registros son ventanas fijas (`docs/spec/hw/v3d.md`, §2.1 y §2.4). Su permiso DMA tiene desplazamiento 0: la
    // V3D ve direcciones físicas.
    // Solo si esa dirección es de aparatos en esta placa (en QEMU `virt` no lo es, U3).
    let puente = if crate::placa::es_periferico(0xFEC1_1000) {
        crate::mmio::RegLe32::new(crate::mmu::virt(0xFEC1_1000) + 0x20).sondear()
    } else {
        None
    };
    if puente == Some(0x6272_6467) {
        for (nombre, fisica, tamano) in [("v3d", 0xFEC0_0000, 0x8000), ("v3d-puente", 0xFEC1_1000, 0x1000), ("pm", 0xFE10_0000, 0x1000), ("cprman", 0xFE10_1000, 0x1000)] {
            apuntar(&mut llaves, nombre, REGISTROS, Llave::nueva(Objeto::Registros { fisica, tamano }, DERECHOS));
        }
        let permiso = Objeto::PermisoDma(alloc::sync::Arc::new(memoria::PermisoDma::nuevo(memoria::Zona::Normal, 0)));
        apuntar(&mut llaves, "dma-gpu", PERMISO_DMA, Llave::nueva(permiso, DERECHOS));
    }
    // El xHCI y el NVMe del PCIe genérico (U6), si los hay: sus registros (la BAR 0) y un permiso DMA propio para
    // cada uno. En QEMU `virt` los aparatos del PCIe alcanzan toda la memoria; se les da la zona de DMA, como a los
    // demás.
    // Con GICv3, el ITS para sus MSI (U6h), si lo hay.
    if d.compatible("pci-host-ecam-generic").is_some() && crate::its::iniciar(&d) {
        ITS_LISTO.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    // El sonido de virtio (A2, S19): virtio 1.2, 4.1.2, fabricante 0x1af4 y aparato 0x1040 más el tipo, 25. Sus
    // registros los dicen sus capacidades de configuración; en QEMU, todos en la BAR 4. El controlador recibe también
    // su espacio de configuración, para leerlas.
    if let Some((fisica, tamano, irq, cfg)) = aparato_pci(&d, Pci::Id(0x1af4, 0x1059), 0x20, "virtio-sonido") {
        apuntar(&mut llaves, "virtio-sonido", REGISTROS, Llave::nueva(Objeto::Registros { fisica, tamano }, DERECHOS));
        apuntar(&mut llaves, "vsonido-cfg", REGISTROS, Llave::nueva(Objeto::Registros { fisica: cfg, tamano: memoria::PAGINA }, DERECHOS));
        if let Some(irq) = irq.and_then(interrupciones::Interrupcion::nueva) {
            apuntar(&mut llaves, "virtio-sonido", INTERRUPCION, Llave::nueva(Objeto::Interrupcion(irq), DERECHOS));
        }
        let permiso = Objeto::PermisoDma(alloc::sync::Arc::new(memoria::PermisoDma::nuevo(memoria::Zona::Dma1G, 0)));
        apuntar(&mut llaves, "dma-vsonido", PERMISO_DMA, Llave::nueva(permiso, DERECHOS));
    }
    for (nombre, dma, clase) in [("xhci", "dma-xhci", 0x0C_0330), ("nvme", "dma-nvme", 0x01_0802)] {
        if let Some((fisica, tamano, irq, _)) = aparato_pci(&d, Pci::Clase(clase), 0x10, nombre) {
            apuntar(&mut llaves, nombre, REGISTROS, Llave::nueva(Objeto::Registros { fisica, tamano }, DERECHOS));
            if let Some(irq) = irq.and_then(interrupciones::Interrupcion::nueva) {
                apuntar(&mut llaves, nombre, INTERRUPCION, Llave::nueva(Objeto::Interrupcion(irq), DERECHOS));
            }
            let permiso = Objeto::PermisoDma(alloc::sync::Arc::new(memoria::PermisoDma::nuevo(memoria::Zona::Dma1G, 0)));
            apuntar(&mut llaves, dma, PERMISO_DMA, Llave::nueva(permiso, DERECHOS));
        }
    }
    // Un servicio al que le falta algún aparato no se lanza (la GPU en QEMU, por ejemplo).
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
