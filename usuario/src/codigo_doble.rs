//! La memoria de código de un JIT en x86-64 (X7a, X8): un objeto de memoria proyectado dos veces, para escribir
//! (`memoria_proyectar`) y para leer y ejecutar (`memoria_proyectar_codigo`), sin que ninguna página se pueda escribir
//! y ejecutar a la vez. Es lo que pide `relevo_code_memory` de RelevoJIT; la usan el programa `relevo` (X7a) y la caja
//! Amiga de x86 (`caja/pegamento/`, X8).
//!
//! Las direcciones las pone quien llama: en lo libre desde 384 GB (`docs/referencia/kernel.md`), lejos del montón
//! (64 GB) y de sus pedidos grandes (480 GB). Una sola memoria viva por par de direcciones.

/// Donde la ponen el programa `relevo` y la caja: para escribir en 384 GB y para ejecutar en 388 GB.
pub const ESCRIBIR_EN: usize = 0x60_0000_0000;
pub const EJECUTAR_EN: usize = 0x61_0000_0000;
/// Lo más que da el kernel de una vez (`memoria_crear`, hasta 16 MB).
pub const TAM_MAX: usize = 16 << 20;

/// Una memoria de código viva: su objeto y sus dos proyecciones.
pub struct CodigoDoble {
    llave: u32,
    escribir: usize,
    ejecutar: usize,
}

impl CodigoDoble {
    /// Un objeto de `tam` bytes, proyectado para escribir en `escribir` y para ejecutar en `ejecutar`. None si el
    /// kernel no lo da (tamaño, memoria, o algo ya proyectado ahí).
    pub fn nueva(tam: usize, escribir: usize, ejecutar: usize) -> Option<CodigoDoble> {
        let m = crate::memoria_crear(tam);
        if (m as i32) <= 0 {
            return None;
        }
        if crate::memoria_proyectar(m, escribir, true) != 0 {
            crate::cerrar(m);
            return None;
        }
        if crate::memoria_proyectar_codigo(m, ejecutar) != 0 {
            crate::memoria_quitar(escribir);
            crate::cerrar(m);
            return None;
        }
        Some(CodigoDoble { llave: m, escribir, ejecutar })
    }

    pub fn escribir(&self) -> usize {
        self.escribir
    }

    pub fn ejecutar(&self) -> usize {
        self.ejecutar
    }

    /// Quita las dos proyecciones y suelta el objeto.
    pub fn quitar(self) {
        crate::memoria_quitar(self.escribir);
        crate::memoria_quitar(self.ejecutar);
        crate::cerrar(self.llave);
    }
}

/// Tras escribir código, antes de ejecutarlo. En x86 no hay caché de instrucciones que limpiar, pero el código se
/// escribió por otra dirección lineal que la de ejecutarlo: una instrucción que serializa, CPUID (Intel SDM, volumen
/// 3, "Self-Modifying Code"; como el paso 37 de `malo`).
pub fn escrito() {
    // SAFETY: CPUID existe en todo x86-64 y solo lee.
    #[allow(unused_unsafe)]
    let _ = unsafe { core::arch::x86_64::__cpuid(0) };
}
