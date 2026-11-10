//! U5, de prueba: hace de `selector`, la ventana de "Abrir", solo en `virt`. Quien le pide un fichero recibe el asa
//! de `C/Privado/nota.txt` (lo que "elige el usuario"), cedida solo para leer: con ella lee ese fichero aunque su ficha
//! no le deje, y nada más. El de verdad enseñará la ventana y cederá lo que el usuario elija.
//!
//! Llaves: 1 el canal "archivos" (con el derecho de repartir), 2 su canal "selector".

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::servicio::{cerrar_llaves, recibir, Recibido, MAX_DATOS, MAX_LLAVES};

dashos::programa!(principal);

const ARCHIVOS: u32 = 1;
const CANAL: u32 = 2;
const ELEGIDO: &str = "C/Privado/nota.txt";

/// Una orden al servicio de archivos: (estado, asa).
fn pedir(lectura: u32, orden: u32, asa: u32, x: u64, y: u64, nombre: &str) -> (i32, u32) {
    let mut p = [0u8; 128];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&asa.to_le_bytes());
    p[8..16].copy_from_slice(&x.to_le_bytes());
    p[16..24].copy_from_slice(&y.to_le_bytes());
    p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    p[32..32 + nombre.len()].copy_from_slice(nombre.as_bytes());
    if dashos::canal_escribir(ARCHIVOS, &p[..32 + nombre.len()], &[]) != 0 {
        return (-1, 0);
    }
    let mut r = [0u8; 32];
    let (n, _) = dashos::canal_leer(lectura, &mut r, &mut []);
    if n != 32 {
        return (-1, 0);
    }
    (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()))
}

fn principal() -> u64 {
    // La sesión con el servicio de archivos.
    let (lectura, suyo) = dashos::canal_crear();
    let mut s = [0u8; a::CABECERA];
    s[0..4].copy_from_slice(&a::SESION.to_le_bytes());
    let mut ll1 = [0u32; 1];
    let mut r = [0u8; 32];
    let sesion = dashos::canal_escribir(ARCHIVOS, &s, &[suyo]) == 0 && dashos::canal_leer(lectura, &mut r, &mut ll1).0 == 32;
    if !sesion {
        println!("  [selector] sin servicio de archivos");
        return 1;
    }
    let mut buf = [0u8; MAX_DATOS];
    let mut ll = [0u32; MAX_LLAVES];
    loop {
        let (k, marca) = match recibir(CANAL, &mut buf, &mut ll) {
            Recibido::Mensaje { k, marca, .. } => (k, marca),
            Recibido::Cerrado => return 0,
            Recibido::Nada => continue,
        };
        if marca != 0 && k >= 1 {
            let (e, mia) = pedir(lectura, a::BUSCAR, 0, 0, 0, ELEGIDO);
            let (estado, suya) = if e == a::BIEN { pedir(lectura, a::CEDER, mia, marca, 4, "") } else { (e, 0) };
            if e == a::BIEN {
                pedir(lectura, a::CERRAR, mia, 0, 0, "");
            }
            println!("  [selector] cede {} solo para leer: {} (asa {})", ELEGIDO, estado, suya);
            let respuesta = if estado == a::BIEN { suya } else { 0 };
            dashos::canal_escribir(ll[0], &respuesta.to_le_bytes(), &[]);
        }
        cerrar_llaves(&ll[..k]);
    }
}
