//! Un árbol de dispositivos (FDT) escrito desde cero, sin montón (U5): para las máquinas que solo dan ACPI.
//!
//! Formato de devicetree 0.4, cap. 5: cabecera de 40 bytes, mapa de reservas (vacío aquí: las reservas las añade
//! después `copiar_arbol`), bloque de estructura con sus tokens y bloque de cadenas con los nombres de propiedad.

const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const END: u32 = 9;

pub struct Fdt {
    est: [u8; 8192],
    n: usize,
    cad: [u8; 1024],
    nc: usize,
}

impl Fdt {
    pub const fn nuevo() -> Self {
        Self { est: [0; 8192], n: 0, cad: [0; 1024], nc: 0 }
    }

    fn bytes(&mut self, b: &[u8]) {
        let fin = (self.n + b.len()).min(self.est.len());
        let k = fin - self.n;
        self.est[self.n..fin].copy_from_slice(&b[..k]);
        self.n = (self.n + b.len()).next_multiple_of(4).min(self.est.len());
    }

    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_be_bytes());
    }

    /// El desplazamiento de un nombre en el bloque de cadenas, añadiéndolo si no está.
    fn cadena(&mut self, nombre: &str) -> u32 {
        let b = nombre.as_bytes();
        let mut i = 0;
        while i < self.nc {
            let fin = self.cad[i..self.nc].iter().position(|&c| c == 0).map_or(self.nc, |p| i + p);
            if &self.cad[i..fin] == b {
                return i as u32;
            }
            i = fin + 1;
        }
        let i = self.nc;
        if i + b.len() + 1 <= self.cad.len() {
            self.cad[i..i + b.len()].copy_from_slice(b);
            self.cad[i + b.len()] = 0;
            self.nc += b.len() + 1;
        }
        i as u32
    }

    pub fn nodo(&mut self, nombre: &str) {
        self.u32(BEGIN_NODE);
        let mut b = [0u8; 64];
        let l = nombre.len().min(63);
        b[..l].copy_from_slice(&nombre.as_bytes()[..l]);
        self.bytes(&b[..l + 1]);
    }

    pub fn fin_nodo(&mut self) {
        self.u32(END_NODE);
    }

    pub fn prop(&mut self, nombre: &str, valor: &[u8]) {
        let off = self.cadena(nombre);
        self.u32(PROP);
        self.u32(valor.len() as u32);
        self.u32(off);
        self.bytes(valor);
    }

    /// Una propiedad de celdas de 32 bits.
    pub fn celdas(&mut self, nombre: &str, v: &[u32]) {
        let mut b = [0u8; 256];
        for (i, x) in v.iter().enumerate().take(64) {
            b[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
        }
        self.prop(nombre, &b[..4 * v.len().min(64)]);
    }

    /// Una propiedad de texto (con su 0 al final).
    pub fn texto(&mut self, nombre: &str, s: &str) {
        let mut b = [0u8; 64];
        let l = s.len().min(63);
        b[..l].copy_from_slice(&s.as_bytes()[..l]);
        self.prop(nombre, &b[..l + 1]);
    }

    /// Escribe el árbol entero en `destino`. Devuelve su tamaño, o `None` si no cabe.
    pub fn escribir(&mut self, destino: &mut [u8]) -> Option<usize> {
        self.u32(END);
        let reservas = 40;
        let estructura = reservas + 16;
        let cadenas = estructura + self.n;
        let total = cadenas + self.nc;
        if total > destino.len() {
            return None;
        }
        let cab = [0xd00d_feed, total as u32, estructura as u32, cadenas as u32, reservas as u32, 17, 16, 0, self.nc as u32, self.n as u32];
        for (i, v) in cab.iter().enumerate() {
            destino[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
        }
        destino[reservas..estructura].fill(0); // mapa de reservas vacío
        destino[estructura..cadenas].copy_from_slice(&self.est[..self.n]);
        destino[cadenas..total].copy_from_slice(&self.cad[..self.nc]);
        Some(total)
    }
}
