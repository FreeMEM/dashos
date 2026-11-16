//! Lo que Paq necesita del disco, por rutas con nombre (`Paquetes:Paq/Generaciones/3`). En DashOS lo da el servicio de
//! archivos (`ordenes::paq::DiscoDashOS`); en las pruebas, `Memoria`, un disco en memoria que se puede cortar en
//! cualquier paso (un corte de luz: desde ahí no se escribe nada más).
//!
//! Los errores son los del servicio de archivos (`dashos::protocolo::archivos`).

use alloc::string::String;
use alloc::vec::Vec;

pub const MAL_FORMADO: i32 = -1;
pub const NO_EXISTE: i32 = -2;
pub const NO_ES_CAJON: i32 = -3;
pub const ES_CAJON: i32 = -4;
pub const ERROR_DEL_DISCO: i32 = -7;
pub const YA_EXISTE: i32 = -9;
pub const NO_VACIO: i32 = -10;
pub const LLENO: i32 = -11;
pub const PROTEGIDO: i32 = -12;

pub trait Disco {
    /// El fichero entero, si no pasa de `tope` bytes (más: `LLENO`).
    fn leer(&mut self, ruta: &str, tope: usize) -> Result<Vec<u8>, i32>;
    /// Un fichero nuevo, entero de una vez: o queda entero o no queda (una transacción del sistema de archivos).
    /// `YA_EXISTE` si ya hay uno. Para lo pequeño: fichas, generaciones, el cerrojo.
    fn guardar(&mut self, ruta: &str, datos: &[u8]) -> Result<(), i32>;
    /// Un fichero nuevo, escrito por trozos, con sus permisos (`rwx` de Unix). No es atómico: se usa dentro de un cajón
    /// que aún no se ve.
    fn escribir(&mut self, ruta: &str, datos: &[u8], modo: u16) -> Result<(), i32>;
    /// Un cajón nuevo (`YA_EXISTE` si ya está).
    fn hacer_cajon(&mut self, ruta: &str) -> Result<(), i32>;
    /// Lo que hay en un cajón: (nombre, si es cajón).
    fn listar(&mut self, ruta: &str) -> Result<Vec<(String, bool)>, i32>;
    /// Cambia de nombre o mueve, en el mismo volumen, sin pisar (`YA_EXISTE`). Atómico.
    fn renombrar(&mut self, de: &str, a: &str) -> Result<(), i32>;
    /// Borra un fichero o un cajón vacío.
    fn borrar(&mut self, ruta: &str) -> Result<(), i32>;
    /// Si existe y si es un cajón.
    fn mirar(&mut self, ruta: &str) -> Result<Option<bool>, i32>;
    /// Lo libre en el volumen de `ruta`, en bytes, si se sabe.
    fn libre(&mut self, ruta: &str) -> Option<u64>;
    /// La hora del calendario (segundos desde 1970), si es creíble.
    fn hora(&mut self) -> Option<u64>;
}

/// `base` y `nombre` juntos: `Paquetes:` + `Paq` es `Paquetes:Paq`; `Home:Paquetes` + `Paq`, `Home:Paquetes/Paq`.
pub fn unir(base: &str, nombre: &str) -> String {
    let mut s = String::from(base);
    if !(s.is_empty() || s.ends_with(':') || s.ends_with('/')) {
        s.push('/');
    }
    s.push_str(nombre);
    s
}

/// El cajón de `ruta` y su último nombre.
pub fn partir(ruta: &str) -> (&str, &str) {
    match ruta.rfind(['/', ':']) {
        Some(i) if ruta.as_bytes()[i] == b'/' => (&ruta[..i], &ruta[i + 1..]),
        Some(i) => (&ruta[..=i], &ruta[i + 1..]),
        None => ("", ruta),
    }
}

/// Borra `ruta` con todo lo que tenga dentro.
pub fn borrar_todo(d: &mut dyn Disco, ruta: &str) -> Result<(), i32> {
    match d.mirar(ruta)? {
        None => Ok(()),
        Some(false) => d.borrar(ruta),
        Some(true) => {
            for (n, _) in d.listar(ruta)? {
                borrar_todo(d, &unir(ruta, &n))?;
            }
            d.borrar(ruta)
        }
    }
}

#[cfg(test)]
pub mod memoria {
    //! Un disco en memoria para las pruebas, sin distinguir mayúsculas (como DashOS). Con `corte`, la operación de
    //! escribir número `corte` y todas las de después fallan, como un corte de luz.
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;

    #[derive(Clone, Debug)]
    pub struct Nodo {
        pub nombre: String,
        pub cajon: bool,
        pub datos: Vec<u8>,
        pub modo: u16,
    }

    #[derive(Clone, Default)]
    pub struct Memoria {
        /// Por la ruta en minúsculas.
        pub nodos: BTreeMap<String, Nodo>,
        /// Volúmenes de solo lectura.
        pub solo_leer: Vec<String>,
        pub escrituras: usize,
        pub corte: Option<usize>,
        pub hora: Option<u64>,
        pub libre: u64,
    }

    fn clave(r: &str) -> String {
        r.to_lowercase()
    }

    impl Memoria {
        pub fn nueva(volumenes: &[&str]) -> Memoria {
            let mut m = Memoria { hora: Some(1_763_294_400), libre: 1 << 30, ..Default::default() };
            for v in volumenes {
                m.nodos.insert(clave(v), Nodo { nombre: v.to_string(), cajon: true, datos: Vec::new(), modo: 0o755 });
            }
            m
        }

        /// Pone un fichero o un cajón a mano (con sus cajones), sin contar como escritura.
        pub fn poner(&mut self, ruta: &str, datos: Option<&[u8]>) {
            let mut hecho = String::new();
            let (vol, resto) = ruta.split_at(ruta.find(':').unwrap() + 1);
            hecho.push_str(vol);
            let partes: Vec<&str> = resto.split('/').filter(|p| !p.is_empty()).collect();
            for (i, p) in partes.iter().enumerate() {
                hecho = unir(&hecho, p);
                let ultimo = i + 1 == partes.len();
                let cajon = !(ultimo && datos.is_some());
                self.nodos.entry(clave(&hecho)).or_insert(Nodo { nombre: p.to_string(), cajon, datos: Vec::new(), modo: 0o644 });
                if ultimo && let Some(d) = datos {
                    self.nodos.get_mut(&clave(&hecho)).unwrap().datos = d.to_vec();
                }
            }
        }

        pub fn ver(&self, ruta: &str) -> Option<&Nodo> {
            self.nodos.get(&clave(ruta))
        }

        fn escribe(&mut self, ruta: &str) -> Result<(), i32> {
            if self.corte.is_some_and(|c| self.escrituras >= c) {
                return Err(ERROR_DEL_DISCO);
            }
            self.escrituras += 1;
            if self.solo_leer.iter().any(|v| clave(ruta).starts_with(&clave(v))) {
                return Err(PROTEGIDO);
            }
            Ok(())
        }

        fn padre_es_cajon(&self, ruta: &str) -> Result<(), i32> {
            let (p, n) = partir(ruta);
            if n.is_empty() || n == "." || n == ".." {
                return Err(MAL_FORMADO);
            }
            match self.nodos.get(&clave(p)) {
                Some(x) if x.cajon => Ok(()),
                Some(_) => Err(NO_ES_CAJON),
                None => Err(NO_EXISTE),
            }
        }

        fn hijos(&self, ruta: &str) -> Vec<String> {
            let pre = clave(&unir(ruta, ""));
            self.nodos.keys().filter(|k| k.starts_with(&pre) && !k[pre.len()..].contains('/') && k.len() > pre.len()).cloned().collect()
        }
    }

    impl Disco for Memoria {
        fn leer(&mut self, ruta: &str, tope: usize) -> Result<Vec<u8>, i32> {
            match self.nodos.get(&clave(ruta)) {
                None => Err(NO_EXISTE),
                Some(n) if n.cajon => Err(ES_CAJON),
                Some(n) if n.datos.len() > tope => Err(LLENO),
                Some(n) => Ok(n.datos.clone()),
            }
        }

        fn guardar(&mut self, ruta: &str, datos: &[u8]) -> Result<(), i32> {
            self.escritura_nueva(ruta, datos, 0o644)
        }

        fn escribir(&mut self, ruta: &str, datos: &[u8], modo: u16) -> Result<(), i32> {
            self.escritura_nueva(ruta, datos, modo)
        }

        fn hacer_cajon(&mut self, ruta: &str) -> Result<(), i32> {
            self.escribe(ruta)?;
            self.padre_es_cajon(ruta)?;
            if self.nodos.contains_key(&clave(ruta)) {
                return Err(YA_EXISTE);
            }
            self.nodos.insert(clave(ruta), Nodo { nombre: partir(ruta).1.to_string(), cajon: true, datos: Vec::new(), modo: 0o755 });
            Ok(())
        }

        fn listar(&mut self, ruta: &str) -> Result<Vec<(String, bool)>, i32> {
            match self.nodos.get(&clave(ruta)) {
                None => return Err(NO_EXISTE),
                Some(n) if !n.cajon => return Err(NO_ES_CAJON),
                _ => {}
            }
            Ok(self.hijos(ruta).iter().map(|k| (self.nodos[k].nombre.clone(), self.nodos[k].cajon)).collect())
        }

        fn renombrar(&mut self, de: &str, a: &str) -> Result<(), i32> {
            self.escribe(a)?;
            if !self.nodos.contains_key(&clave(de)) {
                return Err(NO_EXISTE);
            }
            self.padre_es_cajon(a)?;
            if self.nodos.contains_key(&clave(a)) {
                return Err(YA_EXISTE);
            }
            if clave(a).starts_with(&clave(&unir(de, ""))) {
                return Err(MAL_FORMADO);
            }
            let pre = clave(de);
            let mover: Vec<String> = self.nodos.keys().filter(|k| **k == pre || k.starts_with(&clave(&unir(de, "")))).cloned().collect();
            for k in mover {
                let mut n = self.nodos.remove(&k).unwrap();
                let nueva = alloc::format!("{}{}", clave(a), &k[pre.len()..]);
                if k == pre {
                    n.nombre = partir(a).1.to_string();
                }
                self.nodos.insert(nueva, n);
            }
            Ok(())
        }

        fn borrar(&mut self, ruta: &str) -> Result<(), i32> {
            self.escribe(ruta)?;
            match self.nodos.get(&clave(ruta)) {
                None => return Err(NO_EXISTE),
                Some(n) if n.cajon && !self.hijos(ruta).is_empty() => return Err(NO_VACIO),
                _ => {}
            }
            self.nodos.remove(&clave(ruta));
            Ok(())
        }

        fn mirar(&mut self, ruta: &str) -> Result<Option<bool>, i32> {
            Ok(self.nodos.get(&clave(ruta)).map(|n| n.cajon))
        }

        fn libre(&mut self, _ruta: &str) -> Option<u64> {
            Some(self.libre)
        }

        fn hora(&mut self) -> Option<u64> {
            self.hora
        }
    }

    impl Memoria {
        fn escritura_nueva(&mut self, ruta: &str, datos: &[u8], modo: u16) -> Result<(), i32> {
            self.escribe(ruta)?;
            self.padre_es_cajon(ruta)?;
            if self.nodos.contains_key(&clave(ruta)) {
                return Err(YA_EXISTE);
            }
            if datos.len() as u64 > self.libre {
                return Err(LLENO);
            }
            self.libre -= datos.len() as u64;
            self.nodos.insert(clave(ruta), Nodo { nombre: partir(ruta).1.to_string(), cajon: false, datos: datos.to_vec(), modo });
            Ok(())
        }
    }
}
