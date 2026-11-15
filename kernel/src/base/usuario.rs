//! Copias desde y hacia la memoria del proceso actual (C31, C34).
//!
//! Las hacen `desde_usuario` y `hacia_usuario` (copia.s) con LDTRB/STTRB: la MMU comprueba cada byte con los permisos
//! de EL0, así que con cualquier dirección el resultado es una copia correcta o un error, nunca un acceso del kernel
//! a donde el proceso no puede. Un fallo dentro de la copia vuelve por `copia_fallo` (excepciones.rs). Mientras dura
//! la copia, y solo entonces, la mitad baja es la del proceso (PAN por software).
//!
//! Si toda la memoria del tramo es normal con caché, la copia va por el mapa directo del kernel, sin cambiar de
//! espacio (M6a, `directa`).

unsafe extern "C" {
    fn desde_usuario(destino: *mut u8, origen: u64, n: usize) -> u64;
    fn hacia_usuario(destino: u64, origen: *const u8, n: usize) -> u64;
    static copia_inicio: u8;
    static copia_fin: u8;
    fn copia_fallo();
}

/// Si `elr` es un acceso de las copias de `copia.s`, a dónde hay que volver para que la copia devuelva error (C31).
pub fn fallo_de_copia(elr: u64) -> Option<u64> {
    let (inicio, fin) = unsafe { (&copia_inicio as *const u8 as u64, &copia_fin as *const u8 as u64) };
    (inicio..fin).contains(&elr).then(|| copia_fallo as *const () as u64)
}

/// Fin de la mitad baja: lo que pase de aquí no es del usuario.
const LIMITE: u64 = 1 << 39;

fn con_espacio(origen: u64, n: usize, copia: impl FnOnce() -> u64) -> bool {
    if origen.checked_add(n as u64).is_none_or(|f| f > LIMITE) {
        return false;
    }
    let ttbr0 = super::planificador::ttbr0_actual();
    if ttbr0 == 0 {
        return false;
    }
    super::espacio::activar(ttbr0);
    let r = copia();
    super::espacio::activar_vacio();
    r == 0
}

/// Copia sin cambiar de espacio (M6a): página a página, por el mapa directo del kernel, con el espacio del proceso
/// tomado mientras dura cada página. Cambiar TTBR0 cambia de ASID, y QEMU vacía su TLB entera con cada cambio: eso era
/// el 70 % de una ida y vuelta de un mensaje. Los permisos se miran en la tabla, como los miraría la MMU para EL0.
/// `None` si alguna página no es memoria normal con caché: entonces va por el camino de LDTRB/STTRB.
fn directa(va: u64, n: usize, escribir: bool, mut copiar: impl FnMut(*mut u8, usize, usize)) -> Option<bool> {
    if va.checked_add(n as u64).is_none_or(|f| f > LIMITE) {
        return Some(false);
    }
    let p = super::planificador::proceso_actual_arc()?;
    let mut hecho = 0usize;
    while hecho < n {
        let a = va + hecho as u64;
        let trozo = (n - hecho).min(4096 - (a % 4096) as usize);
        let e = p.espacio.tomar();
        match e.para_copiar(a, escribir) {
            super::espacio::Copia::Directa(k) => copiar(k, hecho, trozo),
            super::espacio::Copia::Falla => return Some(false),
            super::espacio::Copia::Lenta => return None,
        }
        drop(e);
        hecho += trozo;
    }
    Some(true)
}

/// Llena `destino` con la memoria del proceso actual desde `origen`. Falso si no se puede leer entera.
pub fn leer(origen: u64, destino: &mut [u8]) -> bool {
    let n = destino.len();
    let d = destino.as_mut_ptr();
    if let Some(r) = directa(origen, n, false, |k, i, t| unsafe { core::ptr::copy_nonoverlapping(k, d.add(i), t) }) {
        return r;
    }
    con_espacio(origen, n, || unsafe { desde_usuario(d, origen, n) })
}

/// Copia `datos` a la memoria del proceso actual en `destino`. Falso si no se puede escribir entera.
pub fn escribir(destino: u64, datos: &[u8]) -> bool {
    let o = datos.as_ptr();
    if let Some(r) = directa(destino, datos.len(), true, |k, i, t| unsafe { core::ptr::copy_nonoverlapping(o.add(i), k, t) }) {
        return r;
    }
    con_espacio(destino, datos.len(), || unsafe { hacia_usuario(destino, o, datos.len()) })
}
