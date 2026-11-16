//! El historial de lo tecleado: con tope, sin repetir la última, con búsqueda por prefijo y por contenido, y como texto
//! para guardarlo entre sesiones (`ENVARC:Shell/Historial`, una línea por orden).
//!
//! Lo que se lee de un fichero puede venir dañado: bytes que no son texto, líneas enormes, letras de control. Lo que
//! no vale se salta; lo demás sigue.

use alloc::string::String;
use alloc::vec::Vec;

/// Las líneas que se recuerdan.
pub const MAX_LINEAS: usize = 2000;
/// Lo más que mide una línea que se recuerda, en bytes.
pub const MAX_LINEA: usize = crate::MAX_LINEA;

#[derive(Default)]
pub struct Historial {
    lineas: Vec<String>,
}

fn vale(linea: &str) -> bool {
    !linea.trim().is_empty() && linea.len() <= MAX_LINEA && !linea.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f)
}

impl Historial {
    pub fn lineas(&self) -> &[String] {
        &self.lineas
    }

    pub fn len(&self) -> usize {
        self.lineas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lineas.is_empty()
    }

    pub fn borrar(&mut self) {
        self.lineas.clear();
    }

    /// Recuerda una línea tecleada. Dice si hay que guardarla: no, si está vacía, si empieza por espacio (lo que no se
    /// quiere dejar escrito), si no vale o si es la misma que la última.
    pub fn anadir(&mut self, linea: &str) -> bool {
        if linea.starts_with(' ') || !vale(linea) || self.lineas.last().map(String::as_str) == Some(linea) {
            return false;
        }
        if self.lineas.len() >= MAX_LINEAS {
            self.lineas.remove(0);
        }
        self.lineas.push(String::from(linea));
        true
    }

    /// Pone delante lo guardado (el texto de un fichero): las últimas `MAX_LINEAS` líneas que valgan.
    pub fn cargar(&mut self, datos: &[u8]) {
        let mut leidas: Vec<String> = Vec::new();
        for crudo in datos.split(|&b| b == b'\n') {
            let crudo = crudo.strip_suffix(b"\r").unwrap_or(crudo);
            let Ok(l) = core::str::from_utf8(crudo) else { continue };
            if vale(l) && leidas.last().map(String::as_str) != Some(l) {
                leidas.push(String::from(l));
            }
        }
        let sobran = leidas.len().saturating_sub(MAX_LINEAS);
        leidas.drain(..sobran);
        leidas.append(&mut self.lineas);
        let sobran = leidas.len().saturating_sub(MAX_LINEAS);
        leidas.drain(..sobran);
        self.lineas = leidas;
    }

    /// Todo, como texto para un fichero.
    pub fn texto(&self) -> String {
        let mut s = String::new();
        for l in &self.lineas {
            s.push_str(l);
            s.push('\n');
        }
        s
    }

    /// La línea más reciente, anterior a `antes` (o desde el final), que empieza por `prefijo`: su sitio.
    pub fn con_prefijo(&self, prefijo: &str, antes: Option<usize>) -> Option<usize> {
        let hasta = antes.unwrap_or(self.lineas.len()).min(self.lineas.len());
        (0..hasta).rev().find(|&i| self.lineas[i].starts_with(prefijo))
    }

    /// La siguiente tras `despues` que empieza por `prefijo`.
    pub fn con_prefijo_despues(&self, prefijo: &str, despues: usize) -> Option<usize> {
        (despues + 1..self.lineas.len()).find(|&i| self.lineas[i].starts_with(prefijo))
    }

    /// La línea más reciente, anterior a `antes`, que contiene `trozo` (sin distinguir mayúsculas).
    pub fn que_contiene(&self, trozo: &str, antes: Option<usize>) -> Option<usize> {
        let hasta = antes.unwrap_or(self.lineas.len()).min(self.lineas.len());
        let t = trozo.to_lowercase();
        (0..hasta).rev().find(|&i| self.lineas[i].to_lowercase().contains(&t))
    }

    /// Lo que falta de la línea más reciente que empieza por `tecleado` (y es más larga): la sugerencia.
    pub fn sugerencia(&self, tecleado: &str) -> Option<&str> {
        if tecleado.is_empty() {
            return None;
        }
        self.lineas.iter().rev().find(|l| l.len() > tecleado.len() && l.starts_with(tecleado)).map(|l| &l[tecleado.len()..])
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::format;

    #[test]
    fn anadir_y_buscar() {
        let mut h = Historial::default();
        for l in ["Dir SYS:", "Copy a b", "Copy a b", "  ", " secreto", "Dir Home:", "List"] {
            h.anadir(l);
        }
        assert_eq!(h.lineas(), ["Dir SYS:", "Copy a b", "Dir Home:", "List"]);
        assert_eq!(h.con_prefijo("Dir", None), Some(2));
        assert_eq!(h.con_prefijo("Dir", Some(2)), Some(0));
        assert_eq!(h.con_prefijo("Dir", Some(0)), None);
        assert_eq!(h.con_prefijo_despues("Dir", 0), Some(2));
        assert_eq!(h.con_prefijo_despues("Dir", 2), None);
        assert_eq!(h.que_contiene("home", None), Some(2));
        assert_eq!(h.que_contiene("a b", None), Some(1));
        assert_eq!(h.que_contiene("nada", None), None);
        assert_eq!(h.sugerencia("Di"), Some("r Home:"));
        assert_eq!(h.sugerencia("List"), None);
        assert_eq!(h.sugerencia(""), None);
    }

    #[test]
    fn guardar_y_cargar() {
        let mut h = Historial::default();
        h.anadir("uno");
        h.anadir("dos ñ");
        let mut otro = Historial::default();
        otro.anadir("de esta sesión");
        otro.cargar(h.texto().as_bytes());
        assert_eq!(otro.lineas(), ["uno", "dos ñ", "de esta sesión"]);
    }

    #[test]
    fn un_fichero_danado() {
        let mut datos = Vec::new();
        datos.extend_from_slice(b"buena\n\xff\xfe no es texto\n\n\r\ncon\x1bescape\ncon\ttab\n");
        datos.extend_from_slice("x".repeat(MAX_LINEA + 1).as_bytes());
        datos.extend_from_slice(b"\notra buena\r\notra buena\nsin salto al final");
        let mut h = Historial::default();
        h.cargar(&datos);
        assert_eq!(h.lineas(), ["buena", "otra buena", "sin salto al final"]);
        // Basura pura, o nada.
        let mut h = Historial::default();
        h.cargar(&[0u8; 5000]);
        h.cargar(b"");
        assert!(h.is_empty());
    }

    #[test]
    fn con_tope() {
        let mut h = Historial::default();
        let mucho: String = (0..3 * MAX_LINEAS).map(|i| format!("orden {i}\n")).collect();
        h.cargar(mucho.as_bytes());
        assert_eq!(h.len(), MAX_LINEAS);
        assert_eq!(h.lineas()[0], format!("orden {}", 2 * MAX_LINEAS));
        for i in 0..10 {
            h.anadir(&format!("nueva {i}"));
        }
        assert_eq!(h.len(), MAX_LINEAS);
        assert_eq!(h.lineas().last().unwrap(), "nueva 9");
    }
}
