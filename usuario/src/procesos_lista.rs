//! S27, oleada 6: la lista del monitor de procesos de la barra, como el de DMS (`Modules/ProcessList/ProcessListPopout.qml`,
//! `ProcessesView.qml`, `ProcessFilterChips.qml` y `Services/ProcessSort.js`): buscar, filtrar (todos, míos, sistema),
//! ordenar por una columna (otra vez la misma, al revés; los nombres de la A a la Z y los números de más a menos, como
//! DMS), elegir con las flechas y saber cómo se cierra el elegido. Sin dibujo: lo pinta `procesos_vista.rs`.
//!
//! Portado de DMS (DankMaterialShell v2.1.2; MIT, copyright (c) 2025 Avenge Media LLC; aviso entero en
//! `docs/licencias/DankMaterialShell-MIT.txt`). Lo que se ve es lo que da `sesiones` (`MONITOR`): con detalle, lo de la
//! persona de delante y las cajas; lo demás del sistema y lo de otras personas, en dos sumas sin nombres (decisión del
//! usuario del 2026-11-13). Una suma no se elige para cerrar; lo de la persona se cierra con `MATAR`; lo que `MATAR` no
//! deja (un programa del escritorio, una caja del sistema) pasa por la tanda de administrador.
//!
//! Sin nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/procesos_lista.rs`.

#[cfg(test)]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Un proceso (o una suma) tal como se enseña.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fila {
    /// Lo que lo distingue de una medida a otra (la marca; en una suma, su clase).
    pub clave: u64,
    /// Su número de proceso (0 en una suma).
    pub marca: u64,
    pub nombre: String,
    /// La pastilla de una caja ("68K", "LINUX").
    pub pastilla: Option<&'static str>,
    /// CPU en décimas de punto y memoria en bytes.
    pub cpu: u32,
    pub memoria: u64,
    pub usuario: String,
    /// Una suma (el sistema, otras personas): no se cierra ni se elige para cerrar.
    pub suma: bool,
    /// Del sistema (por debajo de las personas) o de la persona de delante.
    pub sistema: bool,
    /// Un programa del escritorio: `MATAR` no lo cierra.
    pub escritorio: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filtro {
    Todos,
    Mios,
    Sistema,
}

pub const FILTROS: [(Filtro, &str); 3] = [(Filtro::Todos, "Todos"), (Filtro::Mios, "Míos"), (Filtro::Sistema, "Sistema")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Columna {
    Nombre,
    Cpu,
    Memoria,
    Usuario,
}

/// Cómo se cierra un proceso: no se puede (una suma), como su dueña (`MATAR`) o con la contraseña de un administrador
/// (la acción "matar" de la tanda).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cerrar {
    NoSe,
    Propio(u64),
    Administrador(u64),
}

pub fn como_cerrar(f: &Fila) -> Cerrar {
    if f.suma || f.marca == 0 {
        Cerrar::NoSe
    } else if f.escritorio || f.sistema {
        Cerrar::Administrador(f.marca)
    } else {
        Cerrar::Propio(f.marca)
    }
}

/// Lo más largo de lo que se busca.
pub const MAX_BUSCA: usize = 32;

/// El estado de la lista: lo que se busca, el filtro, el orden, el elegido y la primera fila que se ve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lista {
    pub busca: String,
    pub filtro: Filtro,
    pub orden: Columna,
    /// De menos a más.
    pub subiendo: bool,
    /// El elegido, por su clave (sigue elegido aunque cambie de sitio al reordenar).
    pub elegida: Option<u64>,
    pub primera: usize,
}

impl Default for Lista {
    fn default() -> Self {
        Lista { busca: String::new(), filtro: Filtro::Todos, orden: Columna::Cpu, subiendo: false, elegida: None, primera: 0 }
    }
}

fn minusculas(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

impl Lista {
    /// Las que se ven de `todas`, en orden: sus índices.
    pub fn ver(&self, todas: &[Fila]) -> Vec<usize> {
        let busca = minusculas(self.busca.trim());
        let mut v: Vec<usize> = (0..todas.len())
            .filter(|&i| {
                let f = &todas[i];
                let pasa = match self.filtro {
                    Filtro::Todos => true,
                    Filtro::Mios => !f.sistema && !f.suma,
                    Filtro::Sistema => f.sistema,
                };
                pasa && (busca.is_empty() || minusculas(&f.nombre).contains(&busca) || minusculas(&f.usuario).contains(&busca) || f.marca.to_string_sin_alloc().contains(&busca))
            })
            .collect();
        let orden = self.orden;
        v.sort_by(|&a, &b| {
            let (x, y) = (&todas[a], &todas[b]);
            let c = match orden {
                Columna::Nombre => minusculas(&x.nombre).cmp(&minusculas(&y.nombre)),
                Columna::Cpu => x.cpu.cmp(&y.cpu),
                Columna::Memoria => x.memoria.cmp(&y.memoria),
                Columna::Usuario => minusculas(&x.usuario).cmp(&minusculas(&y.usuario)),
            };
            // A igualdad, por el nombre y la clave: el orden no baila de una medida a otra.
            let c = if self.subiendo { c } else { c.reverse() };
            c.then_with(|| minusculas(&x.nombre).cmp(&minusculas(&y.nombre))).then(x.clave.cmp(&y.clave))
        });
        v
    }

    /// Ordenar por `c`: si ya era esa, al revés; si no, los textos de la A a la Z y los números de más a menos.
    pub fn ordenar_por(&mut self, c: Columna) {
        if self.orden == c {
            self.subiendo = !self.subiendo;
        } else {
            self.orden = c;
            self.subiendo = matches!(c, Columna::Nombre | Columna::Usuario);
        }
        self.primera = 0;
    }

    pub fn filtrar(&mut self, f: Filtro) -> bool {
        let cambia = self.filtro != f;
        self.filtro = f;
        self.primera = 0;
        cambia
    }

    /// Una letra más en lo que se busca (sin las de control). `true` si cambió.
    pub fn escribir(&mut self, c: char) -> bool {
        if c.is_control() || self.busca.chars().count() >= MAX_BUSCA {
            return false;
        }
        self.busca.push(c);
        self.primera = 0;
        true
    }

    pub fn borrar(&mut self) -> bool {
        self.primera = 0;
        self.busca.pop().is_some()
    }

    /// El sitio del elegido entre las que se ven.
    pub fn sitio_elegida(&self, vistas: &[usize], todas: &[Fila]) -> Option<usize> {
        let e = self.elegida?;
        vistas.iter().position(|&i| todas[i].clave == e)
    }

    /// Las flechas: el elegido baja o sube (sin elegido, el primero), y la lista se mueve para que se vea con `filas`
    /// filas a la vista.
    pub fn mover(&mut self, vistas: &[usize], todas: &[Fila], abajo: bool, filas: usize) {
        if vistas.is_empty() {
            self.elegida = None;
            return;
        }
        let k = match self.sitio_elegida(vistas, todas) {
            None => 0,
            Some(k) if abajo => (k + 1).min(vistas.len() - 1),
            Some(k) => k.saturating_sub(1),
        };
        self.elegida = Some(todas[vistas[k]].clave);
        self.a_la_vista(k, vistas.len(), filas);
    }

    /// Que la fila `k` se vea con `filas` a la vista (y que no quede hueco abajo si hay menos).
    pub fn a_la_vista(&mut self, k: usize, total: usize, filas: usize) {
        let filas = filas.max(1);
        if k < self.primera {
            self.primera = k;
        } else if k >= self.primera + filas {
            self.primera = k + 1 - filas;
        }
        self.primera = self.primera.min(total.saturating_sub(filas));
    }

    /// La rueda: `pasos` filas abajo (o arriba, negativos).
    pub fn rueda(&mut self, pasos: i32, total: usize, filas: usize) {
        let tope = total.saturating_sub(filas.max(1));
        self.primera = (self.primera as i64 + pasos as i64).clamp(0, tope as i64) as usize;
    }

    /// Tras medir otra vez: si el elegido ya no está, ninguno; y la primera, dentro.
    pub fn al_dia(&mut self, vistas: &[usize], todas: &[Fila], filas: usize) {
        if self.sitio_elegida(vistas, todas).is_none() {
            self.elegida = None;
        }
        self.primera = self.primera.min(vistas.len().saturating_sub(filas.max(1)));
    }
}

/// Un número en texto sin `format!` (para buscar por número de proceso).
trait SinAlloc {
    fn to_string_sin_alloc(&self) -> String;
}

impl SinAlloc for u64 {
    fn to_string_sin_alloc(&self) -> String {
        if *self == 0 {
            return String::new();
        }
        let mut v = Vec::new();
        let mut n = *self;
        while n > 0 {
            v.push(b'0' + (n % 10) as u8);
            n /= 10;
        }
        v.reverse();
        String::from_utf8(v).unwrap_or_default()
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn fila(clave: u64, nombre: &str, cpu: u32, memoria: u64, usuario: &str, sistema: bool) -> Fila {
        Fila { clave, marca: clave, nombre: nombre.into(), pastilla: None, cpu, memoria, usuario: usuario.into(), suma: false, sistema, escritorio: false }
    }

    fn todas() -> Vec<Fila> {
        let mut v = alloc::vec![
            fila(65540, "espera", 300, 4 << 20, "ana", false),
            fila(65541, "Shell", 20, 9 << 20, "ana", false),
            fila(12, "caja-amiga", 500, 300 << 20, "sistema", true),
            fila(65542, "barra", 10, 2 << 20, "ana", false),
        ];
        v[3].escritorio = true;
        let mut s = fila(3, "sistema (40)", 120, 50 << 20, "sistema", true);
        s.marca = 0;
        s.suma = true;
        v.push(s);
        let mut o = fila(4, "otras personas (3)", 7, 1 << 20, "otras personas", false);
        o.marca = 0;
        o.suma = true;
        v.push(o);
        v
    }

    fn nombres(l: &Lista, t: &[Fila]) -> Vec<String> {
        l.ver(t).iter().map(|&i| t[i].nombre.clone()).collect()
    }

    #[test]
    fn de_serie_por_cpu_de_mas_a_menos() {
        let t = todas();
        let l = Lista::default();
        assert_eq!(nombres(&l, &t), ["caja-amiga", "espera", "sistema (40)", "Shell", "barra", "otras personas (3)"]);
    }

    #[test]
    fn ordenar_filtrar_y_buscar() {
        let t = todas();
        let mut l = Lista::default();
        l.ordenar_por(Columna::Nombre);
        assert!(l.subiendo);
        assert_eq!(nombres(&l, &t)[0], "barra");
        l.ordenar_por(Columna::Nombre);
        assert_eq!(nombres(&l, &t)[0], "sistema (40)");
        l.ordenar_por(Columna::Memoria);
        assert!(!l.subiendo);
        assert_eq!(nombres(&l, &t)[0], "caja-amiga");
        l.filtrar(Filtro::Mios);
        assert_eq!(nombres(&l, &t), ["Shell", "espera", "barra"]);
        l.filtrar(Filtro::Sistema);
        assert_eq!(nombres(&l, &t), ["caja-amiga", "sistema (40)"]);
        l.filtrar(Filtro::Todos);
        for c in "SHE".chars() {
            assert!(l.escribir(c));
        }
        assert_eq!(nombres(&l, &t), ["Shell"]);
        l.busca.clear();
        for c in "6554".chars() {
            l.escribir(c);
        }
        assert_eq!(nombres(&l, &t).len(), 3, "por número de proceso");
        l.busca.clear();
        l.escribir('o');
        l.escribir('t');
        assert_eq!(nombres(&l, &t), ["otras personas (3)"], "por el usuario o el nombre");
        // Hostil: letras de control, demasiado larga, borrar de más.
        l.busca.clear();
        assert!(!l.escribir('\n') && !l.escribir('\u{7}'));
        for _ in 0..100 {
            l.escribir('x');
        }
        assert_eq!(l.busca.chars().count(), MAX_BUSCA);
        for _ in 0..200 {
            l.borrar();
        }
        assert!(!l.borrar());
    }

    #[test]
    fn elegir_y_desplazar() {
        let t = todas();
        let mut l = Lista::default();
        let v = l.ver(&t);
        l.mover(&v, &t, true, 3);
        assert_eq!(l.elegida, Some(12), "sin elegido, el primero");
        for _ in 0..4 {
            l.mover(&v, &t, true, 3);
        }
        assert_eq!(l.sitio_elegida(&v, &t), Some(4));
        assert_eq!(l.primera, 2, "a la vista");
        for _ in 0..10 {
            l.mover(&v, &t, true, 3);
        }
        assert_eq!(l.sitio_elegida(&v, &t), Some(5));
        assert_eq!(l.primera, 3);
        l.rueda(-100, v.len(), 3);
        assert_eq!(l.primera, 0);
        l.rueda(100, v.len(), 3);
        assert_eq!(l.primera, 3);
        // El elegido desaparece en la medida siguiente: ninguno; menos filas: la primera, dentro.
        let menos: Vec<Fila> = t.iter().filter(|f| f.clave != 4).cloned().take(2).collect();
        let v2 = l.ver(&menos);
        l.al_dia(&v2, &menos, 3);
        assert_eq!((l.elegida, l.primera), (None, 0));
        // Sin filas, mover no elige nada.
        l.mover(&[], &menos, true, 3);
        assert_eq!(l.elegida, None);
    }

    #[test]
    fn como_se_cierra() {
        let t = todas();
        assert_eq!(como_cerrar(&t[0]), Cerrar::Propio(65540));
        assert_eq!(como_cerrar(&t[2]), Cerrar::Administrador(12), "una caja del sistema");
        assert_eq!(como_cerrar(&t[3]), Cerrar::Administrador(65542), "un programa del escritorio");
        assert_eq!(como_cerrar(&t[4]), Cerrar::NoSe, "una suma");
        assert_eq!(como_cerrar(&t[5]), Cerrar::NoSe);
    }
}
