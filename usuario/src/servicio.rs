//! El bucle común de los servicios (E7 de `docs/REVISION_ARQUITECTURA.md`): cada servicio lo tenía escrito a su
//! manera, y cada copia tenía sus fallos (E1, E2, A16).

/// Lo que llega por el canal de un servicio.
pub enum Recibido {
    /// Un mensaje: sus bytes, cuántas llaves y la marca de quien escribió (0: sin marca, de `inicio` o del kernel).
    Mensaje { n: usize, k: usize, marca: u64 },
    /// Nadie más puede escribir en el canal: el servicio puede acabar.
    Cerrado,
    /// No se pudo leer (sin sitio en la tabla de llaves o sin memoria): el mensaje sigue en la cola. Ya se esperó un
    /// poco, para no girar (E2); hay que volver a leer.
    Nada,
}

/// Lo más que trae un mensaje: bytes y llaves (`MAX_DATOS` y `MAX_LLAVES` del kernel).
pub const MAX_DATOS: usize = 256;
pub const MAX_LLAVES: usize = 8;

/// Lee el siguiente mensaje de `canal` en `buf` y `ll`. Los dos, del máximo que deja el kernel: un mensaje que no cabe
/// no se puede leer, se queda el primero de la cola y el servicio ya no atiende a nadie (lo hizo el cliente hostil con
/// `sesiones`, que leía 64 bytes y 4 llaves).
pub fn recibir(canal: u32, buf: &mut [u8; MAX_DATOS], ll: &mut [u32; MAX_LLAVES]) -> Recibido {
    let (n, k, marca) = crate::canal_leer_marca(canal, buf, ll);
    if n == crate::E_CERRADO {
        return Recibido::Cerrado;
    }
    if n < 0 {
        crate::dormir_us(1_000);
        return Recibido::Nada;
    }
    Recibido::Mensaje { n: n as usize, k, marca }
}

/// Cierra las llaves que llegaron y el servicio no usa: si no, se le llena la tabla (E2).
pub fn cerrar_llaves(ll: &[u32]) {
    for &l in ll {
        crate::cerrar(l);
    }
}
