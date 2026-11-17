//! S8 (rama `paq`): lo de Paq en `sesiones`, aparte para tocar poco `sesiones.rs`.
//!
//! - **Las tandas de paquetes.** Las acciones de Paq de una tanda confirmada (`tanda::es_de_paq`: instalar, actualizar,
//!   volver, quitar, limpiar) las hace el servicio `paq`, todas juntas en una generación: se le mandan las líneas
//!   enseñadas, tal cual, por la copia sin marca de su canal que da `inicio` (`CANAL`). Contesta luego por el canal de
//!   `sesiones`, con `MARCA_PAQ`, y entonces sigue la tanda (como con el comprobador).
//! - **Las asignaciones de los paquetes** de todos (`Paquetes:Paq/Asignaciones`, que escribe Paq): se leen al arrancar
//!   y tras cada tanda de paquetes, y van en la tabla de nombres de cada proceso, encima de las de la sesión. Solo las
//!   que llevan a `Paquetes:` y no tapan una del sistema: un paquete no puede cambiar `LIBS:` ni `SYS:` de nadie.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use dashos::asignaciones::{self as asig, Cambio, Tabla};
use dashos::tanda;

/// La copia sin marca del canal del servicio `paq` (0 hasta que llega de `inicio`).
pub static CANAL: AtomicU32 = AtomicU32::new(0);
/// Si lo que se espera (en el hueco del comprobador) es al servicio `paq`.
pub static ESPERANDO: AtomicBool = AtomicBool::new(false);
/// Lo que se espera al servicio `paq` (bajar, comprobar y desempaquetar): más que al comprobador.
pub const PLAZO_US: u64 = 120_000_000;
/// Dónde está el fichero de las asignaciones, desde la raíz del disco (la raíz 0 de `sesiones`).
pub const CAMINO: &[u8] = b"Paquetes/Paq/Asignaciones";
pub const MAX: usize = 1024;
const VA_PAQ: usize = 0x68_0000_0000;
/// La orden al servicio (`ordenes::paq::APLICAR`).
const APLICAR: u32 = 1;

struct Texto(UnsafeCell<([u8; MAX], usize)>);
// SAFETY: solo lo toca el hilo principal de `sesiones`, por `poner` y `cargar`, sin dejar salir referencias.
unsafe impl Sync for Texto {}
static ASIGNACIONES: Texto = Texto(UnsafeCell::new(([0; MAX], 0)));

/// Si `l` vale como asignación de un paquete de todos: `NOMBRE=Paquetes:camino`, una capa, sin tapar un volumen ni una
/// asignación de la sesión.
pub fn linea_valida(l: &[u8]) -> bool {
    let Some(i) = l.iter().position(|&c| c == b'=') else { return false };
    let (nombre, destino) = (&l[..i], &l[i + 1..]);
    let de_sesion = asig::DE_SESION.iter().any(|d| d.split(|&c| c == b'=').next().is_some_and(|n| asig::mismo(n, nombre)));
    asig::nombre_valido(nombre)
        && !de_sesion
        && !asig::VOLUMENES.iter().any(|v| asig::mismo(v, nombre))
        && !asig::mismo(nombre, asig::PROGDIR)
        && destino.starts_with(b"Paquetes:")
        && !destino.contains(&b',')
        && destino.len() <= asig::MAX_CAMINO
}

/// Se queda con las líneas que valen de `texto` (las demás no cuentan; las que empiezan por `#` son comentarios).
/// Devuelve (las que valen, las que no).
pub fn cargar(texto: &[u8]) -> (usize, usize) {
    // SAFETY: ver `Texto`.
    let (b, largo) = unsafe { &mut *ASIGNACIONES.0.get() };
    *largo = 0;
    let (mut bien, mut mal) = (0, 0);
    for l in texto.split(|&c| c == b'\n') {
        let l = l.strip_suffix(b"\r").unwrap_or(l);
        if l.is_empty() || l.starts_with(b"#") {
            continue;
        }
        if !linea_valida(l) || *largo + l.len() + 1 > MAX {
            mal += 1;
            continue;
        }
        b[*largo..*largo + l.len()].copy_from_slice(l);
        b[*largo + l.len()] = b'\n';
        *largo += l.len() + 1;
        bien += 1;
    }
    (bien, mal)
}

/// Pone en `t` las asignaciones de los paquetes de todos (las que caben).
pub fn poner(t: &mut Tabla) {
    // SAFETY: ver `Texto`.
    let (b, largo) = unsafe { &*ASIGNACIONES.0.get() };
    for l in b[..*largo].split(|&c| c == b'\n').filter(|l| !l.is_empty()) {
        let _ = t.asignar(Cambio::Poner, l);
    }
}

/// Manda al servicio `paq` las acciones de Paq seguidas de `acciones` desde la `desde`, de `programa` (usuario `uid`).
/// Contesta por `vuelta` (la copia marcada del canal de `sesiones`). Devuelve cuántas manda, o `None` si no se pudo.
pub fn pedir(acciones: &[tanda::Accion], desde: usize, programa: &[u8; 16], uid: u32, vuelta: u32) -> Option<usize> {
    let canal = CANAL.load(Ordering::Relaxed);
    if canal == 0 {
        return None;
    }
    let mut texto = [0u8; tanda::MAX_TEXTO];
    let mut largo = 0;
    let mut cuantas = 0;
    for a in acciones[desde..].iter().take_while(|a| tanda::es_de_paq(a.nombre())) {
        let (n, p) = (a.nombre().as_bytes(), a.paq());
        if largo + n.len() + 1 + p.len() + 1 > texto.len() {
            return None;
        }
        texto[largo..largo + n.len()].copy_from_slice(n);
        texto[largo + n.len()] = b' ';
        texto[largo + n.len() + 1..largo + n.len() + 1 + p.len()].copy_from_slice(p);
        largo += n.len() + 1 + p.len();
        texto[largo] = b'\n';
        largo += 1;
        cuantas += 1;
    }
    if cuantas == 0 {
        return None;
    }
    let mem = dashos::memoria_crear(largo);
    if (mem as i32) <= 0 {
        return None;
    }
    if dashos::memoria_proyectar(mem, VA_PAQ, true) != 0 {
        dashos::cerrar(mem);
        return None;
    }
    // SAFETY: recién proyectada aquí con `largo` bytes; se quita en seguida.
    unsafe { core::slice::from_raw_parts_mut(VA_PAQ as *mut u8, largo) }.copy_from_slice(&texto[..largo]);
    dashos::memoria_quitar(VA_PAQ);
    let mut m = [0u8; 32];
    m[0..4].copy_from_slice(&APLICAR.to_le_bytes());
    m[4..8].copy_from_slice(&(largo as u32).to_le_bytes());
    m[8..12].copy_from_slice(&uid.to_le_bytes());
    m[12..28].copy_from_slice(programa);
    if dashos::canal_escribir(canal, &m, &[vuelta, mem]) != 0 {
        dashos::cerrar(mem);
        return None;
    }
    ESPERANDO.store(true, Ordering::Relaxed);
    Some(cuantas)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn lineas() {
        assert!(linea_valida(b"SALUDO=Paquetes:Saludo"));
        assert!(linea_valida(b"MUI=Paquetes:MUI/Libs"));
        for mala in [&b"LIBS=Paquetes:Malo"[..], b"SYS=Paquetes:x", b"PROGDIR=Paquetes:x", b"X=SYS:Libs", b"X=Paquetes:a,SYS:C", b"=Paquetes:x", b"X"] {
            assert!(!linea_valida(mala), "{:?}", core::str::from_utf8(mala));
        }
    }
}
