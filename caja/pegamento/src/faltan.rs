//! Lo que falta: cada función de una biblioteca nativa de la caja que se llama y aún no está hecha (`NoImplementada`
//! de exec, dos, graphics, intuition, gadtools, utility o timer), con su biblioteca, su LVO y quién la llamó (la
//! biblioteca de disco o el programa donde estaba la instrucción que llamó: la dirección de vuelta de la pila 68k). La
//! primera vez de cada una se dice por la consola; al acabar cada tarea, un resumen con las más pedidas. Es la lista
//! de lo que hay que hacer para que corra un programa (MUI, Tachyon...).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Cuántas se dicen la primera vez (las demás solo se cuentan).
const DICHAS: usize = 64;

#[derive(Default)]
pub(crate) struct Faltan {
    /// (biblioteca, función) → (veces, LVO, quién la llamó la primera vez).
    pub mapa: BTreeMap<(String, String), (u32, i32, String)>,
    pub llamadas: u32,
    dichas: usize,
    /// Lo de la tarea en marcha (para su resumen).
    de_la_tarea: BTreeMap<(String, String), u32>,
}

/// El LVO de una función por su nombre y su biblioteca (0 si no se sabe).
pub(crate) fn lvo_de(biblioteca: &str, funcion: &str) -> i32 {
    match biblioteca {
        "exec.library" => caja_exec::lvo::por_nombre(funcion).map_or(0, |n| -6 * n as i32),
        "dos.library" => caja_dos::lvo::FUNCIONES
            .iter()
            .position(|(n, _)| *n == funcion)
            .map_or(0, |i| caja_dos::lvo::lvo(caja_dos::lvo::PRIMERA + i as u16)),
        "graphics.library" => caja_intuition::lvo::graphics(funcion).map_or(0, caja_intuition::lvo::lvo),
        "intuition.library" => caja_intuition::lvo::intuition(funcion).map_or(0, caja_intuition::lvo::lvo),
        // Una biblioteca entera que no hay (ni nativa ni de disco): su OpenLibrary.
        _ if funcion == "OpenLibrary" || funcion == "OpenDevice" => 0,
        "gadtools.library" => {
            caja_intuition::lvo::GADTOOLS.iter().position(|(n, _)| *n == funcion).map_or(0, caja_intuition::lvo::lvo)
        }
        _ => 0,
    }
}

impl Faltan {
    /// Apunta una llamada a `funcion` de `biblioteca`, hecha desde `quien`. Devuelve lo que hay que decir por la
    /// consola (la primera vez de cada una, hasta `DICHAS`).
    pub fn apuntar(&mut self, biblioteca: &str, funcion: &str, quien: &str) -> Option<String> {
        self.llamadas += 1;
        let k = (String::from(biblioteca), String::from(funcion));
        *self.de_la_tarea.entry(k.clone()).or_insert(0) += 1;
        let lvo = lvo_de(biblioteca, funcion);
        let e = self.mapa.entry(k).or_insert((0, lvo, String::from(quien)));
        e.0 += 1;
        if e.0 == 1 && self.dichas < DICHAS {
            self.dichas += 1;
            return Some(format!("falta: {} {} (LVO {}), la llama {}", biblioteca, funcion, lvo, quien));
        }
        None
    }

    /// El resumen de la tarea que acaba: cuántas distintas y las más pedidas. `None` si no faltó nada.
    pub fn resumen_de_la_tarea(&mut self) -> Option<String> {
        let t = core::mem::take(&mut self.de_la_tarea);
        if t.is_empty() {
            return None;
        }
        let total: u32 = t.values().sum();
        let mut v: Vec<_> = t.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        let mas: Vec<String> = v.iter().take(12).map(|((b, f), n)| format!("{} {} x{}", b.trim_end_matches(".library"), f, n)).collect();
        Some(format!("faltan {} funciones distintas ({} llamadas): {}", v.len(), total, mas.join(", ")))
    }
}

/// Quién hizo la llamada que está en la pila (la dirección de vuelta `pc`): la biblioteca de disco cuyo código la
/// tiene, o la tarea.
pub(crate) fn quien(c: &super::Caja, pc: u32) -> String {
    use caja_exec::Memoria;
    let m = &c.ex.mem;
    for (seg, nombre) in &c.ramlib_segs {
        // La lista de segmentos (como `caja_dos::hunk::segmentos`, con tope): BPTR al siguiente, tamaño delante.
        let mut b = *seg;
        for _ in 0..1024 {
            if b == 0 || b >= 1 << 30 {
                break;
            }
            let p = b << 2;
            let (Ok(bytes), Ok(sig)) = (m.leer32(p.wrapping_sub(4)), m.leer32(p)) else { break };
            if pc >= p + 4 && pc < (p + 4).saturating_add(bytes.saturating_sub(8)) {
                return format!("{} (+${:x})", nombre, pc - (p + 4));
            }
            b = sig;
        }
    }
    let t = c.ex.tarea_actual();
    let n = m.leer32(t + 10).ok().and_then(|p| c.ex.leer_cadena(p).ok());
    format!("la tarea {} (${:x})", n.unwrap_or_else(|| String::from("?")), pc)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn cuenta_y_resume() {
        let mut f = Faltan::default();
        assert!(f.apuntar("graphics.library", "AreaMove", "x").is_some());
        assert!(f.apuntar("graphics.library", "AreaMove", "x").is_none());
        assert!(f.apuntar("layers.library", "OpenLibrary", "y").is_some());
        let r = f.resumen_de_la_tarea().unwrap();
        assert!(r.starts_with("faltan 2 funciones distintas (3 llamadas): graphics AreaMove x2"), "{}", r);
        assert!(f.resumen_de_la_tarea().is_none());
        assert_eq!(lvo_de("exec.library", "Supervisor"), -30);
    }
}
