//! El programa de inicio (C47): lo primero que corre en EL0 en la imagen normal.
//!
//! El kernel lo lanza en el grupo raíz con las llaves de arranque, las únicas que da él:
//! - la 1, un objeto de memoria con el paquete de arranque (solo lectura);
//! - después, una por aparato: el permiso DMA y, para cada controlador, sus registros y su interrupción.
//!
//! El paquete dice qué es cada llave y trae los ELF de los servicios, para que `inicio` los lance. Formato
//! (little-endian; lo lee `usuario/src/bin/inicio.rs`):
//! - u32 cuántos aparatos; por cada uno, 24 bytes: nombre (16, relleno con 0), tipo (u32: 1 registros,
//!   2 interrupción, 3 permiso DMA) y número de llave (u32);
//! - u32 cuántos programas; por cada uno, 24 bytes: tamaño (u32), qué aparatos le pasa `inicio` (u32: el bit i es
//!   el aparato i de la lista de arriba; sus llaves serán la 1, la 2... del programa, en ese orden) y nombre (16);
//! - los ELF seguidos, cada uno desde un múltiplo de 8.

use crate::dtb::Dtb;
use crate::objetos::{Llave, Objeto, LEER};
use crate::{interrupciones, kprintln, memoria};
use alloc::vec::Vec;

static INICIO: &[u8] = include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/inicio");

/// Los servicios que lanza `inicio`, en orden, con los aparatos que le pasa a cada uno (por nombre y tipo).
static PROGRAMAS: &[(&str, &[u8], &[(&str, u32)])] = &[
    ("consola", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/consola"), &[("registro", REGISTRO), ("serie", REGISTROS)]),
    ("usb", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/usb"), &[("dma", PERMISO_DMA), ("usb", REGISTROS), ("usb", INTERRUPCION)]),
    ("hola", include_bytes!("../../usuario/target/aarch64-unknown-none-softfloat/release/hola"), &[]),
];

const REGISTROS: u32 = 1;
const INTERRUPCION: u32 = 2;
const PERMISO_DMA: u32 = 3;
const REGISTRO: u32 = 4;
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
        let permiso = Objeto::PermisoDma { zona: memoria::Zona::Dma1G, desplazamiento_bus: bus.wrapping_sub(cpu) };
        apuntar(&mut llaves, "dma", PERMISO_DMA, Llave::nueva(permiso, DERECHOS));
    }
    apuntar(&mut llaves, "registro", REGISTRO, Llave::nueva(Objeto::Registro, DERECHOS));
    for &(nombre, compatible, con_irq) in APARATOS {
        let Some(n) = d.compatible(compatible) else { continue };
        let Some((fisica, tamano)) = d.reg_cpu(&n, 0) else { continue };
        let tamano = tamano.div_ceil(memoria::PAGINA) * memoria::PAGINA;
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
    let mut paquete: Vec<u8> = Vec::new();
    paquete.extend_from_slice(&(manifiesto.len() as u32).to_le_bytes());
    for (nombre, tipo, llave) in &manifiesto {
        paquete.extend_from_slice(nombre);
        paquete.extend_from_slice(&tipo.to_le_bytes());
        paquete.extend_from_slice(&llave.to_le_bytes());
    }
    paquete.extend_from_slice(&(PROGRAMAS.len() as u32).to_le_bytes());
    for (nombre, elf, aparatos) in PROGRAMAS {
        let mut mascara = 0u32;
        for (aparato, tipo) in aparatos.iter() {
            if let Some(i) = manifiesto.iter().position(|(n, t, _)| *n == nombre16(aparato) && t == tipo) {
                mascara |= 1 << i;
            }
        }
        paquete.extend_from_slice(&(elf.len() as u32).to_le_bytes());
        paquete.extend_from_slice(&mascara.to_le_bytes());
        paquete.extend_from_slice(&nombre16(nombre));
    }
    for (_, elf, _) in PROGRAMAS {
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
