//! Listas de control mínimas de la V3D: borrar una baldosa a un color y guardarla en memoria (C27; desde C50, en un
//! proceso).
//!
//! Escrito solo con `docs/spec/hw/v3d-listas.md` (V5), variante A (solo render). De momento solo una baldosa (hasta
//! 64 × 64 píxeles), formato `rgba8` lineal. Los paquetes van byte a byte, con los campos en little-endian (§2). Sin
//! montón: cada lista va en un búfer fijo.

/// Formato de guardado `rgba8` (§3.12).
const FORMATO_RGBA8: u128 = 27;

/// Una lista de bytes de tamaño fijo.
pub struct Bytes {
    b: [u8; 128],
    n: usize,
}

impl Bytes {
    const fn nueva() -> Self {
        Self { b: [0; 128], n: 0 }
    }

    fn mas(&mut self, d: &[u8]) {
        self.b[self.n..self.n + d.len()].copy_from_slice(d);
        self.n += d.len();
    }

    pub fn como_bytes(&self) -> &[u8] {
        &self.b[..self.n]
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

pub struct Borrado {
    pub ancho: u32,
    pub alto: u32,
    /// Palabra del color como la define §3.9: R en los bits bajos (`0xAABBGGRR`).
    pub color: u32,
    pub destino: u32,
    pub paso: u32,
    /// VA de la lista genérica de baldosa.
    pub generica: u32,
    /// VA de una página a cero, alineada a 64 bytes (§3.7).
    pub memoria_baldosas: u32,
}

fn le32(x: u32) -> [u8; 4] {
    x.to_le_bytes()
}

/// Guardado de un búfer de baldosa (`STORE_TILE_BUFFER_GENERAL`, 13 bytes).
fn guardar(v: &mut Bytes, bufer: u8, paso: u32, direccion: u32) {
    let campos: u128 = 0x1D | ((bufer as u128) << 8) | if bufer == 8 { 0 } else { (FORMATO_RGBA8 << 20) | ((paso as u128) << 36) };
    v.mas(&campos.to_le_bytes()[..9]);
    v.mas(&le32(direccion));
}

impl Borrado {
    /// La lista genérica de baldosa (§5.2): 17 bytes.
    pub fn lista_generica(&self) -> Bytes {
        let mut v = Bytes::nueva();
        v.mas(&[0x7D, 0x1A]); // TILE_COORDINATES_IMPLICIT, END_OF_LOADS
        guardar(&mut v, 0, self.paso, self.destino);
        v.mas(&[0x1B, 0x12]); // END_OF_TILE_MARKER, RETURN_FROM_SUB_LIST
        v
    }

    /// La RCL (§5.1). `None` si no cabe en una baldosa.
    pub fn rcl(&self) -> Option<Bytes> {
        if self.ancho == 0 || self.alto == 0 || self.ancho > 64 || self.alto > 64 || self.memoria_baldosas % 64 != 0 {
            return None;
        }
        let fin_generica = self.generica + self.lista_generica().len() as u32;
        let (w, h) = (self.ancho as u16, self.alto as u16);
        let mut v = Bytes::nueva();
        // COMMON: un RT, anchura, altura, early-Z desactivado (§3.2, §8.2).
        v.mas(&[0x79, 0x00]);
        v.mas(&w.to_le_bytes());
        v.mas(&h.to_le_bytes());
        v.mas(&[0x40, 0, 0]);
        // CLEAR_COLORS_PART1 del RT 0 (§3.3).
        v.mas(&[0x79, 0x03]);
        v.mas(&le32(self.color));
        v.mas(&[0, 0, 0]);
        // COLOR: 32 bpp, tipo interno rgba8 (§3.4).
        v.mas(&[0x79, 0x81, 0, 0, 0, 0, 0, 0, 0]);
        // ZS_CLEAR_VALUES: plantilla 0, Z 1.0 (§3.5).
        v.mas(&[0x79, 0x02, 0]);
        v.mas(&1.0f32.to_bits().to_le_bytes());
        v.mas(&[0, 0]);
        v.mas(&[0x7E, 0x05]); // TILE_LIST_INITIAL_BLOCK_SIZE
        v.mas(&[0x7B]); // MULTICORE_RENDERING_TILE_LIST_SET_BASE, conjunto 0
        v.mas(&le32(self.memoria_baldosas));
        // SUPERTILE_CFG: supertiles de 1 × 1, cuadro de 1 × 1, una baldosa de ancho y de alto (§3.8).
        v.mas(&[0x7A, 0, 0, 1, 1, 1, 0x10, 0, 0]);
        // Dos baldosas de preparación (errata GFXH-1742); la primera borra el TLB.
        for borrar in [true, false] {
            v.mas(&[0x7C, 0, 0, 0, 0x1A]); // TILE_COORDINATES (0, 0), END_OF_LOADS
            guardar(&mut v, 8, 0, 0); // guardado de relleno: ningún búfer
            if borrar {
                v.mas(&[0x19, 0x03]); // CLEAR_TILE_BUFFERS
            }
            v.mas(&[0x1B]); // END_OF_TILE_MARKER
        }
        v.mas(&[0x13]); // FLUSH_VCD_CACHE
        v.mas(&[0x14]); // START_ADDRESS_OF_GENERIC_TILE_LIST
        v.mas(&le32(self.generica));
        v.mas(&le32(fin_generica));
        v.mas(&[0x17, 0, 0, 0x0D]); // SUPERTILE_COORDINATES (0, 0), END_OF_RENDERING
        Some(v)
    }
}
