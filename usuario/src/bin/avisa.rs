//! S10: un programa del disco (`Programas/avisa`) que avisa. Lo lanza la barra como la persona que está delante: pide
//! la llave de `avisos` a `sesiones` (permiso fijo, `PERMISOS`) y manda un aviso. Llave 1: su canal con `sesiones`.

#![no_std]
#![no_main]

use dashos::avisos_cliente::avisar;
use dashos::println;
use dashos::protocolo::avisos::NORMAL;
use dashos::protocolo::sesiones as s;

dashos::programa!(principal);

const SESIONES: u32 = 1;

fn principal() -> u64 {
    let nombre = b"avisos";
    let mut p = [0u8; 32];
    p[0..4].copy_from_slice(&s::SERVICIO.to_le_bytes());
    p[4..8].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    p[12..12 + nombre.len()].copy_from_slice(nombre);
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(SESIONES, &p[..12 + nombre.len()], &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 10_000_000)
    } else {
        (-1, 0, 0)
    };
    dashos::cerrar(lectura);
    if n != 4 || k != 1 {
        println!("  [avisa] sin la llave de avisos ({})", if n == 4 { i32::from_le_bytes(r) } else { n as i32 });
        return 1;
    }
    let respuesta = avisar(ll[0], NORMAL, "Copia terminada", "Se copiaron 3 ficheros a Home:Documentos");
    match respuesta {
        Some(r) => println!("  [avisa] avisa: {} (número {}, sale como {})", r.estado, r.numero, r.de()),
        None => println!("  [avisa] el servicio de avisos no contesta"),
    }
    0
}
