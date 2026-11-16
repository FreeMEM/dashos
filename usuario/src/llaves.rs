//! Qué es cada llave con que nace un programa: la **tabla de llaves**.
//!
//! Quien lanza un programa (`inicio`, `sesiones`) le da sus llaves 1, 2... y dice al kernel el nombre de cada una
//! (`proceso_crear_con_nombres`); el kernel los guarda con el proceso y el programa los lee con la llamada 56
//! (`Tabla::leer`). Así un programa busca "ventanas" o "selector" por su nombre, sin saber quién lo lanzó ni cuántas
//! llaves trae. El programa no puede cambiar su tabla, y un programa lanzado sin nombres tiene una tabla vacía.
//!
//! Los nombres: 16 bytes por llave, rellenos con ceros, en el orden de las llaves (el primero es el de la llave 1). Un
//! canal se llama como el servicio ("ventanas", "archivos", "sesiones", "selector"); un aparato, como en el paquete
//! de arranque; la copia para escribirse a uno mismo, "yo:" y el canal. Un nombre vacío es una llave sin nombre.
//!
//! Sin dependencias: se prueba en el PC con `rustc --edition 2024 --test usuario/src/llaves.rs`.

/// Lo que mide un nombre.
pub const NOMBRE: usize = 16;
/// Las llaves con que puede nacer un programa, como mucho.
pub const MAX: usize = 8;
/// En la llamada 12 (`proceso_crear`), con las llaves: tras sus números van sus nombres.
pub const CON_NOMBRES: u64 = 1 << 16;

/// La tabla de un programa.
#[derive(Clone, Copy)]
pub struct Tabla {
    cuantas: usize,
    bytes: [u8; NOMBRE * MAX],
}

/// Un nombre en sus 16 bytes (lo que sobre se corta).
pub fn nombre16(n: &str) -> [u8; NOMBRE] {
    let mut b = [0u8; NOMBRE];
    let l = n.len().min(NOMBRE);
    b[..l].copy_from_slice(&n.as_bytes()[..l]);
    b
}

impl Tabla {
    pub const fn vacia() -> Self {
        Self { cuantas: 0, bytes: [0; NOMBRE * MAX] }
    }

    /// De lo que contesta la llamada 56: `dice` (cuántos nombres hay, o un error negativo) y los `datos` que se
    /// copiaron. No se fía de ninguno de los dos: solo cuentan los nombres enteros que hay en `datos`, hasta `MAX` y
    /// hasta los que `dice`.
    pub fn de_bytes(datos: &[u8], dice: i64) -> Self {
        let mut t = Self::vacia();
        if dice <= 0 {
            return t;
        }
        t.cuantas = (dice as u64).min(MAX as u64).min((datos.len() / NOMBRE) as u64) as usize;
        t.bytes[..t.cuantas * NOMBRE].copy_from_slice(&datos[..t.cuantas * NOMBRE]);
        t
    }

    /// Cuántas llaves tienen sitio en la tabla (con nombre o sin él).
    pub fn cuantas(&self) -> usize {
        self.cuantas
    }

    /// El nombre de la llave `llave` (desde 1), hasta su primer cero. Nada si no está en la tabla o no tiene nombre.
    pub fn nombre(&self, llave: u32) -> Option<&[u8]> {
        let i = (llave as usize).checked_sub(1).filter(|&i| i < self.cuantas)?;
        let n = &self.bytes[i * NOMBRE..(i + 1) * NOMBRE];
        let n = &n[..n.iter().position(|&c| c == 0).unwrap_or(NOMBRE)];
        (!n.is_empty()).then_some(n)
    }

    /// La llave que se llama `nombre`: la primera, si hubiera varias. Nada si no hay (o si se busca el nombre vacío, o
    /// uno de más de 16 bytes).
    pub fn buscar(&self, nombre: &str) -> Option<u32> {
        if nombre.is_empty() || nombre.len() > NOMBRE {
            return None;
        }
        (1..=self.cuantas as u32).find(|&l| self.nombre(l) == Some(nombre.as_bytes()))
    }

    /// La tabla de este programa (llamada 56). Vacía si quien lo lanzó no dio nombres.
    #[cfg(not(test))]
    pub fn leer() -> Self {
        let mut b = [0u8; NOMBRE * MAX];
        let dice = crate::llamada(56, [b.as_mut_ptr() as u64, b.len() as u64, 0, 0, 0]).0 as i64;
        Self::de_bytes(&b, dice)
    }
}

/// La llave de este programa que se llama `nombre`, si nació con ella y con su tabla.
#[cfg(not(test))]
pub fn buscar(nombre: &str) -> Option<u32> {
    Tabla::leer().buscar(nombre)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn tabla(nombres: &[&str]) -> ([u8; NOMBRE * MAX], i64) {
        let mut b = [0u8; NOMBRE * MAX];
        for (i, n) in nombres.iter().enumerate() {
            b[i * NOMBRE..(i + 1) * NOMBRE].copy_from_slice(&nombre16(n));
        }
        (b, nombres.len() as i64)
    }

    #[test]
    fn busca_por_nombre() {
        let (b, n) = tabla(&["sesiones", "archivos", "ventanas", "selector"]);
        let t = Tabla::de_bytes(&b, n);
        assert_eq!(t.cuantas(), 4);
        assert_eq!(t.buscar("sesiones"), Some(1));
        assert_eq!(t.buscar("ventanas"), Some(3));
        assert_eq!(t.buscar("selector"), Some(4));
        assert_eq!(t.buscar("red"), None);
        assert_eq!(t.nombre(2), Some(&b"archivos"[..]));
        assert_eq!(t.nombre(0), None);
        assert_eq!(t.nombre(5), None);
    }

    #[test]
    fn sin_tabla_no_hay_nada() {
        let t = Tabla::de_bytes(&[], 0);
        assert_eq!(t.cuantas(), 0);
        assert_eq!(t.buscar("ventanas"), None);
        // Un kernel que no conoce la llamada contesta un error.
        let (b, _) = tabla(&["ventanas"]);
        assert_eq!(Tabla::de_bytes(&b, -1).buscar("ventanas"), None);
    }

    #[test]
    fn truncada_solo_cuenta_lo_que_llego() {
        let (b, _) = tabla(&["ventanas", "archivos", "selector"]);
        // Dice 3 y solo llegaron 2 nombres enteros y medio.
        let t = Tabla::de_bytes(&b[..2 * NOMBRE + 8], 3);
        assert_eq!(t.cuantas(), 2);
        assert_eq!(t.buscar("selector"), None);
        assert_eq!(t.buscar("archivos"), Some(2));
        // Dice menos de los que llegaron: los de más no cuentan.
        let t = Tabla::de_bytes(&b, 1);
        assert_eq!(t.buscar("archivos"), None);
        // Dice una barbaridad: como mucho, MAX y lo que hay.
        let t = Tabla::de_bytes(&b, i64::MAX);
        assert_eq!(t.cuantas(), MAX);
        assert_eq!(t.buscar("selector"), Some(3));
        let t = Tabla::de_bytes(&b[..NOMBRE - 1], 8);
        assert_eq!(t.cuantas(), 0);
    }

    #[test]
    fn nombres_raros() {
        let mut b = [0u8; NOMBRE * MAX];
        // 1: 16 bytes sin cero al final; 2: vacío; 3: con un cero en medio; 4: no es UTF-8; 5 y 6: repetido.
        b[..NOMBRE].copy_from_slice(b"0123456789abcdef");
        b[2 * NOMBRE..2 * NOMBRE + 5].copy_from_slice(b"ab\0cd");
        b[3 * NOMBRE..3 * NOMBRE + 2].copy_from_slice(&[0xff, 0xfe]);
        b[4 * NOMBRE..4 * NOMBRE + 3].copy_from_slice(b"red");
        b[5 * NOMBRE..5 * NOMBRE + 3].copy_from_slice(b"red");
        let t = Tabla::de_bytes(&b, 6);
        assert_eq!(t.buscar("0123456789abcdef"), Some(1));
        assert_eq!(t.buscar("0123456789abcde"), None);
        assert_eq!(t.buscar("0123456789abcdefg"), None);
        assert_eq!(t.nombre(2), None);
        assert_eq!(t.buscar(""), None);
        assert_eq!(t.buscar("ab"), Some(3));
        assert_eq!(t.buscar("ab\0cd"), None);
        assert_eq!(t.nombre(4), Some(&[0xff, 0xfe][..]));
        assert_eq!(t.buscar("red"), Some(5));
    }

    #[test]
    fn nombre16_corta() {
        assert_eq!(&nombre16("ventanas")[..9], b"ventanas\0");
        assert_eq!(&nombre16("un-nombre-larguísimo-de-verdad"), b"un-nombre-largu\xc3");
    }
}
