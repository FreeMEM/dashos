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
    ("usb", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/usb"), &[("dma", PERMISO_DMA), ("usb", REGISTROS), ("usb", INTERRUPCION)], &["entrada"]),
    ("compositor", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/compositor"), &[], &["buzon", "entrada"]),
    ("gpu", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/gpu"), &[("dma-gpu", PERMISO_DMA), ("v3d", REGISTROS), ("v3d-puente", REGISTROS), ("pm", REGISTROS), ("cprman", REGISTROS)], &["buzon"]),
    ("hola", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/hola"), &[], &[]),
];

const REGISTROS: u32 = 1;
const INTERRUPCION: u32 = 2;
const PERMISO_DMA: u32 = 3;
const REGISTRO: u32 = 4;
const FIRMWARE: u32 = 5;
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
    // La V3D (C50): solo si su puente contesta. QEMU no la emula, y leer su ventana daría un fallo de bus: se sondea.
    // Sus registros son ventanas fijas (`docs/spec/hw/v3d.md`, §2.1 y §2.4). Su permiso DMA tiene desplazamiento 0: la
    // V3D ve direcciones físicas.
    let puente = crate::mmio::RegLe32::new(crate::mmu::virt(0xFEC1_1000) + 0x20).sondear();
    if puente == Some(0x6272_6467) {
        for (nombre, fisica, tamano) in [("v3d", 0xFEC0_0000, 0x8000), ("v3d-puente", 0xFEC1_1000, 0x1000), ("pm", 0xFE10_0000, 0x1000), ("cprman", 0xFE10_1000, 0x1000)] {
            apuntar(&mut llaves, nombre, REGISTROS, Llave::nueva(Objeto::Registros { fisica, tamano }, DERECHOS));
        }
        let permiso = Objeto::PermisoDma(alloc::sync::Arc::new(memoria::PermisoDma::nuevo(memoria::Zona::Normal, 0)));
        apuntar(&mut llaves, "dma-gpu", PERMISO_DMA, Llave::nueva(permiso, DERECHOS));
    }
    // Un servicio al que le falta algún aparato no se lanza (la GPU en QEMU, por ejemplo).
    let lanzables: Vec<_> = PROGRAMAS
        .iter()
        .filter(|(nombre, _, aparatos, _)| {
            let falta = aparatos.iter().find(|(a, t)| !manifiesto.iter().any(|(n, tm, _)| *n == nombre16(a) && tm == t));
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
            if let Some(i) = manifiesto.iter().position(|(n, t, _)| *n == nombre16(aparato) && t == tipo) {
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
