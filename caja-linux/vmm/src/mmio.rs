//! Lo que tienen en común los aparatos de MMIO: un acceso del invitado (desplazamiento dentro del aparato, tamaño
//! y, si escribe, el valor) y el nivel de su línea de interrupción.

use crate::memoria::MemoriaInvitado;

/// Qué clase de aparato es, para describirlo en el árbol de dispositivos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clase {
    Pl011,
    VirtioMmio,
}

pub trait AparatoMmio {
    /// Tamaño de su ventana de MMIO.
    fn tam_region(&self) -> u64;
    fn clase(&self) -> Clase;
    /// Una lectura de `tam` bytes (1, 2, 4 u 8) en el desplazamiento `desp`. Lo que no existe se lee como 0.
    fn leer(&mut self, desp: u64, tam: u8, mem: &mut dyn MemoriaInvitado) -> u64;
    /// Una escritura. Lo que no existe se ignora.
    fn escribir(&mut self, desp: u64, tam: u8, valor: u64, mem: &mut dyn MemoriaInvitado);
    /// Nivel de su línea de interrupción (todas son por nivel).
    fn irq(&self) -> bool;
    /// Mira si su lado de DashOS tiene algo para el invitado (teclas para la consola...) y lo atiende. El servicio lo
    /// llama cuando le llega algo por sus canales, no en cada salida.
    fn sondear(&mut self, _mem: &mut dyn MemoriaInvitado) {}
}

/// Tamaño de acceso válido.
pub fn tam_valido(tam: u8) -> bool {
    matches!(tam, 1 | 2 | 4 | 8)
}

/// Lo que se queda de `valor` con `tam` bytes.
pub fn recortar(valor: u64, tam: u8) -> u64 {
    match tam {
        1 => valor & 0xff,
        2 => valor & 0xffff,
        4 => valor & 0xffff_ffff,
        _ => valor,
    }
}
