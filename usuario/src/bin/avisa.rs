//! S10: un programa del disco (`Programas/avisa`) que avisa. Lo lanza la barra como la persona que está delante: pide
//! la llave de `avisos` a `sesiones` (permiso fijo, `PERMISOS`) y manda un aviso. Llave 1: su canal con `sesiones`.

#![no_std]
#![no_main]

use dashos::avisos_cliente::{avisar_con, oir_accion};
use dashos::println;
use dashos::protocolo::avisos::{NORMAL, PROTEGIDO};
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
    // S27: un aviso con acciones (la de por defecto, "Abrir", y dos con botón), con un canal suyo para oírlas.
    let (oigo, escucha) = dashos::canal_crear();
    let acciones = [("default", "Abrir"), ("abrir", "Abrir carpeta"), ("deshacer", "Deshacer")];
    let respuesta = avisar_con(ll[0], NORMAL, "Copia terminada", "Se copiaron 3 ficheros a Home:Documentos", 0, &acciones, Some(escucha));
    let numero = match respuesta {
        Some(r) => {
            println!("  [avisa] avisa: {} (número {}, sale como {}, {} acciones)", r.estado, r.numero, r.de(), acciones.len());
            r.numero
        }
        None => {
            println!("  [avisa] el servicio de avisos no contesta");
            return 1;
        }
    };
    // Hostil: sustituir el aviso de antes (de otro programa) no se puede. Si es el primero, no hay de otro.
    if numero > 1 {
        let ajeno = avisar_con(ll[0], NORMAL, "Copia terminada", "secuestro", numero - 1, &[], None);
        println!("  [avisa] sustituir el aviso {} (de otro): {}", numero - 1, match ajeno.map(|r| r.estado) {
            Some(PROTEGIDO) => "no se deja: bien",
            _ => "MAL",
        });
    } else {
        println!("  [avisa] es el primer aviso: no hay de otro que sustituir");
    }
    // Espera a que se pulse una de sus acciones (en el emergente o en el centro), hasta 90 s.
    let mut id = [0u8; 16];
    match oir_accion(oigo, 90_000_000, &mut id) {
        Some((n, l)) => println!("  [avisa] acción pulsada: {} (aviso {}){}", core::str::from_utf8(&id[..l]).unwrap_or("?"), n,
            if n == numero { "" } else { ": de otro aviso, MAL" }),
        None => println!("  [avisa] nadie pulsó ninguna acción"),
    }
    0
}
