//! Copias desde y hacia la memoria del proceso actual (C31, C34).
//!
//! Las hacen `desde_usuario` y `hacia_usuario` (copia.s) con LDTRB/STTRB: la MMU comprueba cada byte con los permisos
//! de EL0, así que con cualquier dirección el resultado es una copia correcta o un error, nunca un acceso del kernel
//! a donde el proceso no puede. Un fallo dentro de la copia vuelve por `copia_fallo` (excepciones.rs). Mientras dura
//! la copia, y solo entonces, la mitad baja es la del proceso (PAN por software).

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

/// Llena `destino` con la memoria del proceso actual desde `origen`. Falso si no se puede leer entera.
pub fn leer(origen: u64, destino: &mut [u8]) -> bool {
    let n = destino.len();
    con_espacio(origen, n, || unsafe { desde_usuario(destino.as_mut_ptr(), origen, n) })
}

/// Copia `datos` a la memoria del proceso actual en `destino`. Falso si no se puede escribir entera.
pub fn escribir(destino: u64, datos: &[u8]) -> bool {
    con_espacio(destino, datos.len(), || unsafe { hacia_usuario(destino, datos.as_ptr(), datos.len()) })
}
