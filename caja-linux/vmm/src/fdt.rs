//! Un escritor de árboles de dispositivos aplanados (Devicetree 0.4, cap. 5), con `alloc`.
//!
//! La misma idea que `uefi/src/fdt.rs` (que no tiene montón): un bloque de estructura con sus tokens y un bloque de
//! cadenas con los nombres de propiedad sin repetir. Aquí además hay mapa de reservas.

use alloc::vec::Vec;

const MAGIA: u32 = 0xd00d_feed;
const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const END: u32 = 9;

#[derive(Default)]
pub struct EscritorFdt {
    estructura: Vec<u8>,
    cadenas: Vec<u8>,
    reservas: Vec<(u64, u64)>,
    abiertos: usize,
    phandle: u32,
}

impl EscritorFdt {
    pub fn nuevo() -> Self {
        Self::default()
    }

    fn u32(&mut self, v: u32) {
        self.estructura.extend_from_slice(&v.to_be_bytes());
    }

    fn rellenar(&mut self) {
        while !self.estructura.len().is_multiple_of(4) {
            self.estructura.push(0);
        }
    }

    fn cadena(&mut self, nombre: &str) -> u32 {
        let b = nombre.as_bytes();
        let mut i = 0;
        while i < self.cadenas.len() {
            let fin = self.cadenas[i..].iter().position(|&c| c == 0).map_or(self.cadenas.len(), |p| i + p);
            if &self.cadenas[i..fin] == b {
                return i as u32;
            }
            i = fin + 1;
        }
        let i = self.cadenas.len();
        self.cadenas.extend_from_slice(b);
        self.cadenas.push(0);
        i as u32
    }

    /// Un phandle nuevo (1, 2, 3...), para ponerlo con `prop_u32("phandle", h)`.
    pub fn nuevo_phandle(&mut self) -> u32 {
        self.phandle += 1;
        self.phandle
    }

    /// Una entrada del mapa de reservas (memoria que el invitado no debe usar).
    pub fn reservar(&mut self, dir: u64, tam: u64) {
        self.reservas.push((dir, tam));
    }

    pub fn nodo(&mut self, nombre: &str) {
        self.u32(BEGIN_NODE);
        self.estructura.extend_from_slice(nombre.as_bytes());
        self.estructura.push(0);
        self.rellenar();
        self.abiertos += 1;
    }

    pub fn fin_nodo(&mut self) {
        self.u32(END_NODE);
        self.abiertos = self.abiertos.saturating_sub(1);
    }

    pub fn prop(&mut self, nombre: &str, valor: &[u8]) {
        let off = self.cadena(nombre);
        self.u32(PROP);
        self.u32(valor.len() as u32);
        self.u32(off);
        self.estructura.extend_from_slice(valor);
        self.rellenar();
    }

    pub fn prop_vacia(&mut self, nombre: &str) {
        self.prop(nombre, &[]);
    }

    pub fn prop_u32(&mut self, nombre: &str, v: u32) {
        self.prop(nombre, &v.to_be_bytes());
    }

    pub fn prop_u64(&mut self, nombre: &str, v: u64) {
        self.prop(nombre, &v.to_be_bytes());
    }

    pub fn prop_celdas(&mut self, nombre: &str, celdas: &[u32]) {
        let b: Vec<u8> = celdas.iter().flat_map(|c| c.to_be_bytes()).collect();
        self.prop(nombre, &b);
    }

    /// Una lista de pares (dirección, tamaño) de 64 bits (#address-cells = #size-cells = 2).
    pub fn prop_reg(&mut self, nombre: &str, pares: &[(u64, u64)]) {
        let b: Vec<u8> = pares.iter().flat_map(|&(d, t)| [d.to_be_bytes(), t.to_be_bytes()]).flatten().collect();
        self.prop(nombre, &b);
    }

    pub fn prop_texto(&mut self, nombre: &str, s: &str) {
        self.prop_textos(nombre, &[s]);
    }

    /// Una lista de textos, cada uno con su 0 (como `compatible`).
    pub fn prop_textos(&mut self, nombre: &str, textos: &[&str]) {
        let mut b = Vec::new();
        for t in textos {
            b.extend_from_slice(t.as_bytes());
            b.push(0);
        }
        self.prop(nombre, &b);
    }

    /// El árbol entero. Cierra los nodos que queden abiertos.
    pub fn terminar(mut self, boot_cpuid: u32) -> Vec<u8> {
        while self.abiertos > 0 {
            self.fin_nodo();
        }
        self.u32(END);
        let reservas = 40usize; // tras la cabecera, alineado a 8
        let tam_reservas = 16 * (self.reservas.len() + 1);
        let estructura = reservas + tam_reservas;
        let cadenas = estructura + self.estructura.len();
        let total = cadenas + self.cadenas.len();
        let mut out = Vec::with_capacity(total);
        for v in [
            MAGIA,
            total as u32,
            estructura as u32,
            cadenas as u32,
            reservas as u32,
            17,
            16,
            boot_cpuid,
            self.cadenas.len() as u32,
            self.estructura.len() as u32,
        ] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        for &(d, t) in self.reservas.iter().chain(core::iter::once(&(0, 0))) {
            out.extend_from_slice(&d.to_be_bytes());
            out.extend_from_slice(&t.to_be_bytes());
        }
        out.extend_from_slice(&self.estructura);
        out.extend_from_slice(&self.cadenas);
        out
    }
}

/// Un lector mínimo, para las pruebas: recorre el árbol y da (ruta, propiedad, valor).
#[cfg(test)]
pub fn leer(fdt: &[u8]) -> Vec<(alloc::string::String, alloc::string::String, Vec<u8>)> {
    use alloc::string::String;
    let be = |o: usize| u32::from_be_bytes(fdt[o..o + 4].try_into().unwrap()) as usize;
    assert_eq!(be(0) as u32, MAGIA);
    assert_eq!(be(4), fdt.len());
    let (est, cad) = (be(8), be(12));
    let mut ruta: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut o = est;
    loop {
        let tok = be(o) as u32;
        o += 4;
        match tok {
            BEGIN_NODE => {
                let fin = fdt[o..].iter().position(|&c| c == 0).unwrap();
                ruta.push(String::from_utf8(fdt[o..o + fin].to_vec()).unwrap());
                o = (o + fin + 1).next_multiple_of(4);
            }
            END_NODE => {
                ruta.pop();
            }
            PROP => {
                let (largo, nombre) = (be(o), be(o + 4));
                o += 8;
                let fin = fdt[cad + nombre..].iter().position(|&c| c == 0).unwrap();
                let n = String::from_utf8(fdt[cad + nombre..cad + nombre + fin].to_vec()).unwrap();
                let r = if ruta.len() <= 1 { String::from("/") } else { ruta[1..].join("/") };
                out.push((r, n, fdt[o..o + largo].to_vec()));
                o = (o + largo).next_multiple_of(4);
            }
            END => return out,
            otro => panic!("token {otro}"),
        }
    }
}
