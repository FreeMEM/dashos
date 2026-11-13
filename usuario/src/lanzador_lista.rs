//! S27: la lista del lanzador de la barra, sin dibujo: los programas, lo que se busca, los que casan (primero los que
//! empiezan así, luego los que lo contienen, sin distinguir mayúsculas), cuál está elegido y qué trozo se ve.
//!
//! Se prueba en el PC con `rustc --edition 2024 --test usuario/src/lanzador_lista.rs`.

#[cfg(test)]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Lo más que se lista de `Programas/` (más, y el lanzador no los enseña: se dice en el registro).
pub const MAX_PROGRAMAS: usize = 1000;
/// Lo que se escribe en el buscador, como mucho (en letras).
pub const MAX_BUSCA: usize = 30;

#[derive(Default)]
pub struct Lista {
    pub programas: Vec<String>,
    /// En minúsculas, para buscar sin rehacerlas a cada tecla.
    minusculas: Vec<String>,
    pub busca: String,
    /// Los que casan con la búsqueda (por su sitio en `programas`).
    pub vistos: Vec<usize>,
    pub elegido: usize,
    /// La primera fila que se ve.
    pub primero: usize,
    /// Cuántas filas caben.
    pub filas: usize,
}

impl Lista {
    /// Pone los programas (ordenados por nombre, sin mayúsculas; como mucho `MAX_PROGRAMAS`) y vacía la búsqueda.
    pub fn poner(&mut self, mut programas: Vec<String>, filas: usize) {
        programas.truncate(MAX_PROGRAMAS);
        programas.sort_by_cached_key(|p| p.to_lowercase());
        self.minusculas = programas.iter().map(|p| p.to_lowercase()).collect();
        self.programas = programas;
        self.filas = filas.max(1);
        self.busca.clear();
        self.filtrar();
    }

    pub fn filtrar(&mut self) {
        let b = self.busca.to_lowercase();
        let mut empiezan = Vec::new();
        let mut contienen = Vec::new();
        for (i, n) in self.minusculas.iter().enumerate() {
            if n.starts_with(&b) {
                empiezan.push(i);
            } else if n.contains(&b) {
                contienen.push(i);
            }
        }
        empiezan.extend(contienen);
        self.vistos = empiezan;
        self.elegido = 0;
        self.primero = 0;
    }

    /// Una letra más en la búsqueda (las de control y las que pasan del tope, no). Devuelve si cambió.
    pub fn escribir(&mut self, c: char) -> bool {
        if c.is_control() || self.busca.chars().count() >= MAX_BUSCA {
            return false;
        }
        self.busca.push(c);
        self.filtrar();
        true
    }

    pub fn borrar(&mut self) -> bool {
        let hubo = self.busca.pop().is_some();
        if hubo {
            self.filtrar();
        }
        hubo
    }

    /// Elegir el de abajo o el de arriba; el trozo que se ve sigue a lo elegido.
    pub fn mover(&mut self, abajo: bool) {
        if self.vistos.is_empty() {
            return;
        }
        self.elegido = if abajo { (self.elegido + 1).min(self.vistos.len() - 1) } else { self.elegido.saturating_sub(1) };
        if self.elegido < self.primero {
            self.primero = self.elegido;
        } else if self.elegido >= self.primero + self.filas {
            self.primero = self.elegido + 1 - self.filas;
        }
    }

    /// El programa de la fila `k` de las que se ven (por su sitio en `vistos`), si la hay.
    pub fn en_fila(&self, k: usize) -> Option<usize> {
        let i = self.primero + k;
        (k < self.filas && i < self.vistos.len()).then_some(i)
    }

    pub fn elegido_nombre(&self) -> Option<&str> {
        self.vistos.get(self.elegido).map(|&i| self.programas[i].as_str())
    }

    /// Los nombres que casan, para el registro (los primeros 8, y cuántos más).
    pub fn nombres(&self) -> String {
        if self.vistos.is_empty() {
            return String::from("ninguno");
        }
        let mut s = String::new();
        for (k, &i) in self.vistos.iter().take(8).enumerate() {
            if k > 0 {
                s.push(' ');
            }
            s.push_str(&self.programas[i]);
        }
        if self.vistos.len() > 8 {
            s.push_str(&alloc::format!(" y {} más", self.vistos.len() - 8));
        }
        s
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::format;

    fn lista(nombres: &[&str], filas: usize) -> Lista {
        let mut l = Lista::default();
        l.poner(nombres.iter().map(|n| String::from(*n)).collect(), filas);
        l
    }

    #[test]
    fn buscar_y_elegir() {
        let mut l = lista(&["workbench", "Hola", "avisa", "cambio", "shell", "hostil"], 3);
        assert_eq!(l.nombres(), "avisa cambio Hola hostil shell workbench");
        l.escribir('h');
        // Primero los que empiezan por h, luego los que la contienen.
        assert_eq!(l.nombres(), "Hola hostil shell workbench");
        l.escribir('o');
        assert_eq!(l.nombres(), "Hola hostil");
        l.mover(true);
        assert_eq!(l.elegido_nombre(), Some("hostil"));
        l.borrar();
        l.borrar();
        assert!(!l.borrar(), "vacía: nada que borrar");
        assert_eq!(l.vistos.len(), 6);
        for _ in 0..10 {
            l.mover(true);
        }
        assert_eq!((l.elegido, l.primero), (5, 3));
        assert_eq!(l.en_fila(2), Some(5));
        assert_eq!(l.en_fila(3), None);
        l.escribir('x');
        assert_eq!((l.nombres().as_str(), l.elegido_nombre()), ("ninguno", None));
        l.mover(true);
        assert_eq!(l.en_fila(0), None);
    }

    /// Hostil: miles de programas con nombres enormes y raros, búsquedas largas, letras de control y moverse de más.
    #[test]
    fn miles_y_nombres_enormes() {
        let mut nombres: Vec<String> = (0..5000).map(|i| format!("{}{}", "ñ".repeat(120), i)).collect();
        nombres.push(String::from("\u{202e}al revés"));
        nombres.push(String::new());
        let mut l = Lista::default();
        l.poner(nombres, 7);
        assert_eq!(l.programas.len(), MAX_PROGRAMAS);
        for _ in 0..100 {
            l.escribir('ñ');
        }
        assert_eq!(l.busca.chars().count(), MAX_BUSCA);
        assert!(!l.escribir('\u{7}'));
        assert_eq!(l.vistos.len(), MAX_PROGRAMAS);
        for _ in 0..20_000 {
            l.mover(true);
        }
        assert_eq!(l.elegido, MAX_PROGRAMAS - 1);
        assert!(l.elegido >= l.primero && l.elegido < l.primero + l.filas);
        for _ in 0..20_000 {
            l.mover(false);
        }
        assert_eq!((l.elegido, l.primero), (0, 0));
        assert!(l.nombres().ends_with(&format!(" y {} más", MAX_PROGRAMAS - 8)));
        // Sin filas que quepan, al menos una.
        l.poner(alloc::vec![String::from("a")], 0);
        assert_eq!(l.filas, 1);
    }
}
