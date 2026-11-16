//! `Abrir`: abre un programa instalado como lo abre el usuario con doble clic, **encerrado** (S6b, perfil
//! "aplicación"; S36, decisión 1), para probarlo como lo verá: con su propia identidad (sus permisos, no los del
//! Shell), solo su cajón `Home:Datos/<programa>` (que ve como `Datos:`), `SYS:` y su `PROGDIR:`, sin `Home:`, `Work:`
//! ni `RAM:`, y sin la capa del usuario en `C:`, `LIBS:`, `FONTS:` y `CLASSES:`, salvo que esté en modo de desarrollo
//! (`Permiso DAR <programa> desarrollo`). Lo que se lanza escribiendo su nombre en el Shell, en cambio, lleva la ficha
//! del Shell.
//!
//! `Abrir editor`, `Abrir Programas:editor` o `Abrir SYS:Programas/editor`; con `ESPERAR`, la orden no acaba hasta que
//! acabe el programa. Solo lo instalado (`SYS:Programas`, el `Paquetes:` de hoy): un programa suelto no tiene aún
//! identidad propia (S6b, perfil "suelto"), y abierto con la de otro se llevaría sus permisos.

use crate::{di, Orden};
use alloc::string::String;
use dashos::protocolo::sesiones as s;

/// Lo más que se espera con `ESPERAR` (un programa que no acaba no deja colgado el guion).
const MAX_ESPERA_US: u64 = 300_000_000;

/// El camino en el disco de lo que se pide (`Programas/<nombre>`), o `None` si no es un programa instalado.
pub fn camino_de(pedido: &str) -> Option<String> {
    let nombre = if let Some((vol, resto)) = pedido.split_once(':') {
        if vol.eq_ignore_ascii_case("Programas") {
            resto
        } else if vol.eq_ignore_ascii_case("SYS") {
            let (cajon, n) = resto.split_once('/')?;
            if !cajon.eq_ignore_ascii_case("Programas") {
                return None;
            }
            n
        } else {
            return None;
        }
    } else {
        pedido
    };
    let camino = alloc::format!("Programas/{}", nombre);
    dashos_lanzables_vale(&camino).then_some(camino)
}

/// Lo mismo que mira `sesiones` (`lanzables::de_programas`): un nombre sin cajones, sin subir y sin letras de control.
fn dashos_lanzables_vale(camino: &str) -> bool {
    let Some(n) = camino.strip_prefix("Programas/") else { return false };
    !n.is_empty() && n.len() <= 30 && n != "." && n != ".." && !n.bytes().any(|c| c == b'/' || c == b':' || c < 0x20 || c == 0x7f)
}

fn pedir(m: &[u8]) -> Option<[u8; 12]> {
    let (lectura, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(crate::SESIONES, m, &[suyo]) != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return None;
    }
    let mut r = [0u8; 256];
    let (n, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], 20_000_000);
    dashos::cerrar(lectura);
    (n >= 12).then(|| r[..12].try_into().unwrap())
}

/// Si el proceso `pid` sigue (en la lista de procesos de su persona).
pub fn vivo(pid: u64) -> bool {
    let mut desde = 0u64;
    for _ in 0..64 {
        let mut m = [0u8; 12];
        m[0..4].copy_from_slice(&s::PROCESOS.to_le_bytes());
        m[4..12].copy_from_slice(&desde.to_le_bytes());
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0u8; 256];
        let n = if dashos::canal_escribir(crate::SESIONES, &m, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        if n < 8 {
            return true;
        }
        let van = u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize;
        let total = u32::from_le_bytes(r[4..8].try_into().unwrap()) as u64;
        for i in 0..van.min(s::POR_PAGINA) {
            let o = 8 + s::FICHA * i;
            if u64::from_le_bytes(r[o..o + 8].try_into().unwrap()) == pid {
                return true;
            }
        }
        desde += van as u64;
        if van == 0 || desde >= total {
            return false;
        }
    }
    true
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::ABRIR);
    let Some(pedido) = args.texto("PROGRAMA").map(String::from) else {
        o.error(format_args!("falta el programa: Abrir <programa>"));
        return o.fin(shell::RC_ERROR);
    };
    let Some(camino) = camino_de(&pedido) else {
        o.error(format_args!("{}: solo se abren encerrados los programas instalados (SYS:Programas); uno suelto, aún no", pedido));
        return o.fin(shell::RC_ERROR);
    };
    // Que exista, antes de pedirlo: así el error es el del fichero (y no se confunde con el de `sesiones`).
    if let Err(e) = o.s.mirar(&alloc::format!("SYS:{}", camino)) {
        o.error_de(e, &pedido);
        return o.fin(shell::RC_ERROR);
    }
    let mut m = [0u8; 12 + 64];
    m[0..4].copy_from_slice(&s::ABRIR.to_le_bytes());
    m[4..8].copy_from_slice(&(camino.len() as u32).to_le_bytes());
    m[12..12 + camino.len()].copy_from_slice(camino.as_bytes());
    let Some(r) = pedir(&m[..12 + camino.len()]) else {
        o.error(format_args!("sesiones no contesta"));
        return o.fin(shell::RC_ERROR);
    };
    let (estado, pid) = (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[4..12].try_into().unwrap()));
    let corto = &camino["Programas/".len()..];
    if estado != 0 {
        o.error(format_args!("{}: no se abre ({}): un programa encerrado no abre otros, y hace falta tu sesión abierta", corto, estado));
        return o.fin(shell::RC_ERROR);
    }
    di!(o, "Abierto {} (proceso {}), encerrado: solo ve SYS:, su PROGDIR: y su cajón Home:Datos/{}.\n", corto, pid, corto);
    if args.si("ESPERAR") {
        o.vaciar();
        let hasta = dashos::ahora_us() + MAX_ESPERA_US;
        while vivo(pid) {
            if dashos::ahora_us() > hasta {
                o.error(format_args!("{} sigue abierto tras {} s", corto, MAX_ESPERA_US / 1_000_000));
                return o.fin(shell::RC_AVISO);
            }
            dashos::dormir_us(200_000);
        }
        di!(o, "{} acabó.\n", corto);
    }
    o.fin(shell::RC_BIEN)
}
