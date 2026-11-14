//! Copias desde y hacia la memoria del proceso actual en x86-64 (S21, X4), como `../usuario.rs` en ARM (C31, C34).
//!
//! Las hace `copiar_usuario_x86` (vectores.s) con `rep movsb`, entre `stac` y `clac` si hay SMAP. Mientras dura la
//! copia, y solo entonces, la mitad baja es la del proceso (PAN por software, como en ARM). La CPU comprueba cada
//! página con los permisos del kernel y CR0.WP: el kernel no escribe en una página de solo lectura del proceso; y la
//! mitad alta nunca entra (`LIMITE`). Un fallo dentro de la copia vuelve por `copia_fallo_x86` (excepciones.rs) y la
//! copia devuelve error.

unsafe extern "C" {
    fn copiar_usuario_x86(destino: *mut u8, origen: *const u8, n: usize) -> u64;
    static copia_inicio_x86: u8;
    static copia_fin_x86: u8;
    fn copia_fallo_x86();
}

/// Si `rip` es el acceso de la copia, a dónde hay que volver para que devuelva error (C31).
pub fn fallo_de_copia(rip: u64) -> Option<u64> {
    let (inicio, fin) = unsafe { (&copia_inicio_x86 as *const u8 as u64, &copia_fin_x86 as *const u8 as u64) };
    (inicio..fin).contains(&rip).then(|| copia_fallo_x86 as *const () as u64)
}

/// Fin de la mitad baja: lo que pase de aquí no es del usuario (PML4[0], 512 GB, como en ARM).
const LIMITE: u64 = 1 << 39;

fn con_espacio(direccion: u64, n: usize, copia: impl FnOnce() -> u64) -> bool {
    if direccion.checked_add(n as u64).is_none_or(|f| f > LIMITE) {
        return false;
    }
    let cr3 = super::planificador::ttbr0_actual();
    if cr3 == 0 {
        return false;
    }
    super::espacio::activar(cr3);
    let r = copia();
    super::espacio::activar_vacio();
    r == 0
}

/// Llena `destino` con la memoria del proceso actual desde `origen`. Falso si no se puede leer entera.
pub fn leer(origen: u64, destino: &mut [u8]) -> bool {
    let n = destino.len();
    con_espacio(origen, n, || unsafe { copiar_usuario_x86(destino.as_mut_ptr(), origen as *const u8, n) })
}

/// Copia `datos` a la memoria del proceso actual en `destino`. Falso si no se puede escribir entera.
pub fn escribir(destino: u64, datos: &[u8]) -> bool {
    con_espacio(destino, datos.len(), || unsafe { copiar_usuario_x86(destino as *mut u8, datos.as_ptr(), datos.len()) })
}
