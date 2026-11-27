//! La chip RAM vista por los chips (DMA): direcciones de bytes del 68k y palabras de 16 bits en el orden del 68k. Y,
//! si quiere, la imagen que pinta Denise ([`crate::denise`]).
/// Lo que da la caja para que los chips lean y escriban la chip RAM, y para recibir la imagen.
pub trait MemoriaChip {
    /// La palabra en `dir` (par). Fuera de la chip RAM, 0.
    fn leer_palabra(&mut self, dir: u32) -> u16;
    /// Escribe la palabra en `dir` (par). Fuera de la chip RAM, nada.
    fn escribir_palabra(&mut self, dir: u32, valor: u16);
    /// Si quiere la imagen. Si no (lo de serie), Denise no pinta.
    fn quiere_video(&self) -> bool {
        false
    }
    /// Una línea acabada de la imagen: la fila `fila` (0 a [`crate::denise::ALTO`] - 1) del cuadro en curso, con
    /// [`crate::denise::ANCHO`] píxeles `0x00RRGGBB`.
    fn linea_de_video(&mut self, _fila: usize, _pixeles: &[u32]) {}
    /// Ha acabado un cuadro (un campo, en entrelazado): todas sus filas han llegado ya. `largo`: era de 313 líneas
    /// (`LOF`).
    fn cuadro_de_video(&mut self, _largo: bool) {}
}
/// Sin chip RAM: lee 0 y no escribe. Para los accesos que no la necesitan.
pub struct SinMemoria;
impl MemoriaChip for SinMemoria {
    fn leer_palabra(&mut self, _dir: u32) -> u16 { 0 }
    fn escribir_palabra(&mut self, _dir: u32, _valor: u16) {}
}
