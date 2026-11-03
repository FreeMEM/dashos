//! Lectura del árbol de dispositivos (FDT) que deja el firmware en x0.
//!
//! El formato FDT es big-endian; el kernel es little-endian desde C33, así que los números se leen con `from_be`. Sin memoria dinámica: cada
//! consulta recorre el árbol entero, que es pequeño.

const MAGIA: u32 = 0xd00d_feed;
const NODO_INICIO: u32 = 1;
const NODO_FIN: u32 = 2;
const PROPIEDAD: u32 = 3;
const NADA: u32 = 4;
const FIN: u32 = 9;

const MAX_PROF: usize = 16;

#[derive(Clone, Copy)]
pub struct Dtb {
    bytes: &'static [u8],
    estructura: usize,
    cadenas: usize,
    tamano: usize,
}

/// Un nodo encontrado: su desplazamiento en el bloque de estructura y lo necesario para entender su `reg`.
#[derive(Clone, Copy)]
pub struct Nodo {
    pub inicio: usize,
    pub prof: usize,
    /// #address-cells y #size-cells del padre: describen el `reg` de este nodo.
    pub celdas_dir: u32,
    pub celdas_tam: u32,
    /// #address-cells y #size-cells que declara el propio nodo para sus hijos (y su `dma-ranges`).
    pub propias_dir: u32,
    pub propias_tam: u32,
    /// Cadena de `ranges` de los antepasados, para traducir direcciones a direcciones de la CPU.
    rangos: [Rango; MAX_PROF],
}

#[derive(Clone, Copy, Default)]
struct Rango {
    /// Desplazamiento de la propiedad `ranges` (0 si no hay, o si está vacía y es 1:1).
    datos: usize,
    largo: usize,
    celdas_hijo: u32,
    celdas_padre: u32,
    celdas_tam: u32,
}

fn alinear4(x: usize) -> usize {
    (x + 3) & !3
}

impl Dtb {
    /// El árbol que dejó el firmware (la dirección de x0 al arrancar).
    pub fn desde() -> Option<Self> {
        let bytes = crate::base::arbol_de_dispositivos()?;
        let d = Self { bytes, estructura: 0, cadenas: 0, tamano: 0 };
        if d.u32(0) != MAGIA {
            return None;
        }
        Some(Self { bytes, tamano: bytes.len(), estructura: d.u32(8) as usize, cadenas: d.u32(12) as usize })
    }

    pub fn tamano(&self) -> usize {
        self.tamano
    }

    // Fuera del árbol se lee 0: un árbol mal formado da respuestas raras, no accesos fuera.
    fn u8(&self, off: usize) -> u8 {
        self.bytes.get(off).copied().unwrap_or(0)
    }

    fn u32(&self, off: usize) -> u32 {
        match self.bytes.get(off..off.saturating_add(4)) {
            Some(b) => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
            None => 0,
        }
    }

    /// Longitud de la cadena terminada en cero que empieza en `off`.
    fn largo_cadena(&self, off: usize) -> usize {
        let mut n = 0;
        while self.u8(off + n) != 0 {
            n += 1;
        }
        n
    }

    fn igual(&self, off: usize, texto: &str) -> bool {
        let b = texto.as_bytes();
        for (i, c) in b.iter().enumerate() {
            if self.u8(off + i) != *c {
                return false;
            }
        }
        self.u8(off + b.len()) == 0
    }

    /// Copia una cadena del árbol a `buf` y la devuelve como `&str`.
    #[allow(dead_code)]
    pub fn cadena<'a>(&self, off: usize, buf: &'a mut [u8]) -> &'a str {
        let n = self.largo_cadena(off).min(buf.len());
        for (i, b) in buf.iter_mut().enumerate().take(n) {
            *b = self.u8(off + i);
        }
        core::str::from_utf8(&buf[..n]).unwrap_or("?")
    }

    /// Recorre el árbol. `f` recibe cada nodo y su nombre (desplazamiento de la cadena); si devuelve true, se
    /// para y se devuelve ese nodo.
    fn recorrer(&self, mut f: impl FnMut(&Self, &Nodo, usize) -> bool) -> Option<Nodo> {
        let mut off = self.estructura;
        let mut prof = 0usize;
        // Celdas de cada nivel (lo que declara el nodo de ese nivel para sus hijos).
        let mut celdas_dir = [2u32; MAX_PROF + 1];
        let mut celdas_tam = [1u32; MAX_PROF + 1];
        let mut rangos = [Rango::default(); MAX_PROF];
        let mut pendiente: Option<(Nodo, usize)> = None;

        loop {
            let token = self.u32(off);
            off += 4;
            match token {
                NODO_INICIO => {
                    if let Some((mut n, nombre)) = pendiente.take() {
                        n.propias_dir = celdas_dir[prof];
                        n.propias_tam = celdas_tam[prof];
                        if f(self, &n, nombre) {
                            return Some(n);
                        }
                    }
                    let nombre = off;
                    off = alinear4(off + self.largo_cadena(off) + 1);
                    prof += 1;
                    if prof > MAX_PROF {
                        return None;
                    }
                    celdas_dir[prof] = 2;
                    celdas_tam[prof] = 1;
                    rangos[prof - 1] = Rango::default();
                    let nodo = Nodo {
                        inicio: off,
                        prof,
                        celdas_dir: celdas_dir[prof - 1],
                        celdas_tam: celdas_tam[prof - 1],
                        propias_dir: 2,
                        propias_tam: 1,
                        rangos,
                    };
                    pendiente = Some((nodo, nombre));
                }
                PROPIEDAD => {
                    let largo = self.u32(off) as usize;
                    let nombre = self.cadenas + self.u32(off + 4) as usize;
                    let datos = off + 8;
                    if self.igual(nombre, "#address-cells") {
                        celdas_dir[prof] = self.u32(datos);
                    } else if self.igual(nombre, "#size-cells") {
                        celdas_tam[prof] = self.u32(datos);
                    } else if self.igual(nombre, "ranges") && prof >= 1 {
                        // FDT pone las propiedades antes que los hijos, y dtc pone #address-cells antes que ranges.
                        rangos[prof - 1] = Rango {
                            datos,
                            largo,
                            celdas_hijo: celdas_dir[prof],
                            celdas_padre: celdas_dir[prof - 1],
                            celdas_tam: celdas_tam[prof],
                        };
                    }
                    off = alinear4(datos + largo);
                }
                NODO_FIN => {
                    if let Some((mut n, nombre)) = pendiente.take() {
                        n.propias_dir = celdas_dir[prof];
                        n.propias_tam = celdas_tam[prof];
                        if f(self, &n, nombre) {
                            return Some(n);
                        }
                    }
                    if prof == 0 {
                        return None;
                    }
                    prof -= 1;
                }
                NADA => {}
                FIN => return None,
                _ => return None,
            }
        }
    }

    /// Busca una propiedad del nodo. Devuelve (desplazamiento de los datos, largo).
    pub fn propiedad(&self, nodo: &Nodo, nombre: &str) -> Option<(usize, usize)> {
        let mut off = nodo.inicio;
        loop {
            match self.u32(off) {
                PROPIEDAD => {
                    let largo = self.u32(off + 4) as usize;
                    let n = self.cadenas + self.u32(off + 8) as usize;
                    let datos = off + 12;
                    if self.igual(n, nombre) {
                        return Some((datos, largo));
                    }
                    off = alinear4(datos + largo);
                }
                NADA => off += 4,
                _ => return None,
            }
        }
    }

    /// ¿Contiene la lista `compatible` del nodo el texto dado?
    pub fn es_compatible(&self, nodo: &Nodo, texto: &str) -> bool {
        let Some((datos, largo)) = self.propiedad(nodo, "compatible") else { return false };
        let mut off = datos;
        while off < datos + largo {
            if self.igual(off, texto) {
                return true;
            }
            off += self.largo_cadena(off) + 1;
        }
        false
    }

    /// El primer nodo compatible con `texto`.
    pub fn compatible(&self, texto: &str) -> Option<Nodo> {
        self.recorrer(|d, n, _| d.es_compatible(n, texto))
    }

    /// El primer nodo cuyo nombre empieza por `prefijo` (por ejemplo "memory").
    pub fn por_nombre(&self, prefijo: &str) -> Option<Nodo> {
        self.recorrer(|d, _, nombre| {
            let b = prefijo.as_bytes();
            b.iter().enumerate().all(|(i, c)| d.u8(nombre + i) == *c)
        })
    }

    fn leer_celdas(&self, off: usize, celdas: u32) -> u64 {
        let mut v = 0u64;
        for i in 0..celdas as usize {
            v = (v << 32) | self.u32(off + 4 * i) as u64;
        }
        v
    }

    /// Entrada `i` de `reg`, como (dirección del bus del padre, tamaño).
    pub fn reg(&self, nodo: &Nodo, i: usize) -> Option<(u64, u64)> {
        let (datos, largo) = self.propiedad(nodo, "reg")?;
        let paso = 4 * (nodo.celdas_dir + nodo.celdas_tam) as usize;
        if paso == 0 || (i + 1) * paso > largo {
            return None;
        }
        let off = datos + i * paso;
        Some((self.leer_celdas(off, nodo.celdas_dir), self.leer_celdas(off + 4 * nodo.celdas_dir as usize, nodo.celdas_tam)))
    }

    /// Traduce una dirección del bus del padre del nodo a dirección de la CPU, subiendo por los `ranges`.
    pub fn traducir(&self, nodo: &Nodo, mut dir: u64) -> u64 {
        for nivel in (0..nodo.prof.saturating_sub(1)).rev() {
            let r = nodo.rangos[nivel];
            if r.largo == 0 {
                continue; // sin ranges o ranges vacío: 1:1
            }
            let paso = 4 * (r.celdas_hijo + r.celdas_padre + r.celdas_tam) as usize;
            if paso == 0 {
                continue;
            }
            let mut off = r.datos;
            while off + paso <= r.datos + r.largo {
                let hijo = self.leer_celdas(off, r.celdas_hijo);
                let padre = self.leer_celdas(off + 4 * r.celdas_hijo as usize, r.celdas_padre);
                let tam = self.leer_celdas(off + 4 * (r.celdas_hijo + r.celdas_padre) as usize, r.celdas_tam);
                if dir >= hijo && dir < hijo + tam {
                    dir = dir - hijo + padre;
                    break;
                }
                off += paso;
            }
        }
        dir
    }

    /// Dirección de la CPU y tamaño de la entrada `i` de `reg`.
    pub fn reg_cpu(&self, nodo: &Nodo, i: usize) -> Option<(u64, u64)> {
        let (dir, tam) = self.reg(nodo, i)?;
        Some((self.traducir(nodo, dir), tam))
    }

    /// Ventana `i` de `dma-ranges` del nodo: (dirección vista por el periférico, dirección de la CPU, tamaño).
    /// La dirección de la CPU se traduce además por los `ranges` de los antepasados.
    pub fn ventana_dma(&self, nodo: &Nodo, i: usize) -> Option<(u64, u64, u64)> {
        let (datos, largo) = self.propiedad(nodo, "dma-ranges")?;
        let (h, p, t) = (nodo.propias_dir, nodo.celdas_dir, nodo.propias_tam);
        let paso = 4 * (h + p + t) as usize;
        if paso == 0 || (i + 1) * paso > largo {
            return None;
        }
        let off = datos + i * paso;
        let bus = self.leer_celdas(off, h);
        let cpu = self.leer_celdas(off + 4 * h as usize, p);
        let tam = self.leer_celdas(off + 4 * (h + p) as usize, t);
        Some((bus, self.traducir(nodo, cpu), tam))
    }

    /// Entrada `i` del mapa de memoria reservada de la cabecera (`/memreserve/`): (dirección, tamaño).
    pub fn reserva(&self, i: usize) -> Option<(u64, u64)> {
        let mapa = self.u32(16) as usize + 16 * i;
        let dir = ((self.u32(mapa) as u64) << 32) | self.u32(mapa + 4) as u64;
        let tam = ((self.u32(mapa + 8) as u64) << 32) | self.u32(mapa + 12) as u64;
        if dir == 0 && tam == 0 { None } else { Some((dir, tam)) }
    }

    /// Llama a `f` con cada nodo del árbol.
    pub fn cada_nodo(&self, mut f: impl FnMut(&Nodo)) {
        self.recorrer(|_, n, _| {
            f(n);
            false
        });
    }

    /// Celdas `i` de una propiedad cualquiera, como u32.
    pub fn celda(&self, datos: usize, i: usize) -> u32 {
        self.u32(datos + 4 * i)
    }
}
