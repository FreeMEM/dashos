//! La chip RAM vista por los chips (DMA): direcciones de bytes del 68k y palabras de 16 bits en el orden del 68k.
/// Lo que da la caja para que los chips lean y escriban la chip RAM.
pub trait MemoriaChip {
    /// La palabra en `dir` (par). Fuera de la chip RAM, 0.
    fn leer_palabra(&mut self, dir: u32) -> u16;
    /// Escribe la palabra en `dir` (par). Fuera de la chip RAM, nada.
    fn escribir_palabra(&mut self, dir: u32, valor: u16);
}
/// Sin chip RAM: lee 0 y no escribe. Para los accesos que no la necesitan.
pub struct SinMemoria;
impl MemoriaChip for SinMemoria {
    fn leer_palabra(&mut self, _dir: u32) -> u16 { 0 }
    fn escribir_palabra(&mut self, _dir: u32, _valor: u16) {}
}
