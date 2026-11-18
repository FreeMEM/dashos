//! El fichero de ajustes del fondo: `Escritorio/fondo` de `ENV:` (en uso) y de `ENVARC:` (guardado), en el formato de
//! siempre (`clave = valor`, `idioma::texto`). Lo escriben la herramienta de fondos y la orden `Fondo`; editarlo a mano
//! vale igual. Lo que no se entiende se queda con lo de serie y se avisa; nunca rompe el fondo.
//!
//! ```text
//! fondo = Home:Fondos/ondas.png      el de todas las pantallas (un nombre suelto es de Home:Fondos)
//! oscuro = SYS:Fondos/noche.png      el del modo oscuro (si no hay, el mismo)
//! fondo-DASHOS-2 = otro.png          el de una pantalla (por su nombre de salida)
//! oscuro-DASHOS-2 = otro-noche.png
//! ajuste = llenar                    llenar, encajar, centrar, mosaico o estirar
//! color = 204060                     el de las bandas y lo transparente
//! transicion = circulo               ninguna, fundido, barrido, circulo, disolver o azar
//! duracion = 900                     ms de la transición (de 100 a 5000)
//! pase = 0                           minutos entre fondo y fondo (0, sin pase)
//! carpeta = Home:Fondos              de dónde salen los del pase
//! efecto = ninguno                   encima de una imagen: luciernagas, nieve o aurora
//! cpu = 5                            tope de CPU de un fondo animado, % de un núcleo (de 1 a 50), con el compositor
//! ```
//!
//! Un fondo animado es un nombre `animado:aurora` (y los demás de `animado::NOMBRES`).

use super::Ajuste;
use crate::idioma::texto::Ajustes;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const FICHERO: &str = "Escritorio/fondo";
pub const CABECERA: &str = "El fondo de escritorio (Ajustes > Apariencia > Fondos; la orden `Fondo`). Este fichero manda.\nfondo y oscuro: una imagen (Home:Fondos/x.png, SYS:Fondos/x.png) o un fondo animado (animado:aurora); fondo-<pantalla> y oscuro-<pantalla>, el de una pantalla.\najuste: llenar, encajar, centrar, mosaico o estirar. transicion: ninguna, fundido, barrido, circulo, disolver o azar.\npase: minutos entre fondo y fondo de la carpeta (0, sin pase). efecto: ninguno, luciernagas, nieve o aurora. cpu: tope de un fondo animado (% de un núcleo).";
pub const DE_SERIE: &str = "Home:Fondos/ondas.png";
pub const MAX_NOMBRE: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub fondo: String,
    pub oscuro: Option<String>,
    /// Por pantalla: (salida, claro, oscuro).
    pub pantallas: Vec<(String, Option<String>, Option<String>)>,
    pub ajuste: Ajuste,
    pub color: u32,
    pub transicion: String,
    pub duracion: u32,
    pub pase: u32,
    pub carpeta: String,
    pub efecto: Option<String>,
    /// Tope de CPU en milésimas de un núcleo.
    pub cpu: u32,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            fondo: DE_SERIE.to_string(),
            oscuro: None,
            pantallas: Vec::new(),
            ajuste: Ajuste::Llenar,
            color: 0xff20_4060,
            transicion: "circulo".to_string(),
            duracion: 900,
            pase: 0,
            carpeta: "Home:Fondos".to_string(),
            efecto: None,
            cpu: super::ritmo::TOPE_DE_SERIE,
        }
    }
}

/// Un nombre de fondo que vale: un camino de imagen (`Vol:Cajón/x`, o un nombre suelto) o `animado:<nombre>`. Sin
/// subir (`..`), sin letras de control y no muy largo.
pub fn nombre_vale(n: &str) -> bool {
    if n.is_empty() || n.len() > MAX_NOMBRE || n.chars().any(char::is_control) || n.starts_with('/') {
        return false;
    }
    if let Some(a) = n.strip_prefix("animado:") {
        return super::animado::NOMBRES.contains(&a);
    }
    let (vol, resto) = n.split_once(':').unwrap_or(("", n));
    !resto.is_empty() && !vol.contains('/') && resto.split('/').all(|p| !p.is_empty() && p != "." && p != "..") && !resto.contains(':')
}

/// El camino entero de un nombre (un nombre suelto es de `Home:Fondos`).
pub fn camino(n: &str) -> String {
    if n.contains(':') { n.to_string() } else { format!("Home:Fondos/{}", n) }
}

fn nombre(x: &Ajustes, clave: &str, avisos: &mut Vec<String>) -> Option<String> {
    let v = x.valor(clave)?.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("ninguno") {
        return None;
    }
    if nombre_vale(v) {
        Some(camino(v))
    } else {
        avisos.push(format!("{}: {:?} no vale como fondo", clave, v));
        None
    }
}

/// La configuración de su texto, y lo que no se entendió (se queda lo de serie en eso).
pub fn de_texto(bytes: &[u8]) -> (Config, Vec<String>) {
    let mut c = Config::default();
    let mut avisos = Vec::new();
    let x = match Ajustes::leer(bytes) {
        Ok(x) => x,
        Err(_) => {
            avisos.push("no es un fichero de ajustes (texto de 64 KB como mucho)".to_string());
            return (c, avisos);
        }
    };
    if let Some(f) = nombre(&x, "fondo", &mut avisos) {
        c.fondo = f;
    }
    c.oscuro = nombre(&x, "oscuro", &mut avisos);
    let mut salidas: Vec<String> = Vec::new();
    for (k, _) in x.todos() {
        for pre in ["fondo-", "oscuro-"] {
            if let Some(s) = k.strip_prefix(pre).filter(|s| !s.is_empty()) {
                if !salidas.iter().any(|o| o.eq_ignore_ascii_case(s)) && salidas.len() < 16 {
                    salidas.push(s.to_string());
                }
            }
        }
    }
    for s in salidas {
        let claro = nombre(&x, &format!("fondo-{}", s), &mut avisos);
        let oscuro = nombre(&x, &format!("oscuro-{}", s), &mut avisos);
        if claro.is_some() || oscuro.is_some() {
            c.pantallas.push((s, claro, oscuro));
        }
    }
    if let Some(v) = x.valor("ajuste") {
        match Ajuste::de_nombre(v) {
            Some(a) => c.ajuste = a,
            None => avisos.push(format!("ajuste: {:?} no es ninguno", v)),
        }
    }
    if let Some(v) = x.valor("color") {
        match (v.trim().len() == 6).then(|| u32::from_str_radix(v.trim(), 16).ok()).flatten() {
            Some(col) => c.color = 0xff00_0000 | col,
            None => avisos.push(format!("color: {:?} no es un color (seis cifras hexadecimales)", v)),
        }
    }
    if let Some(v) = x.valor("transicion") {
        let v = v.trim().to_ascii_lowercase();
        if super::transicion::Clase::NOMBRES.contains(&v.as_str()) {
            c.transicion = v;
        } else {
            avisos.push(format!("transicion: {:?} no es ninguna", v));
        }
    }
    let numero = |clave: &str, min: u32, max: u32, avisos: &mut Vec<String>| -> Option<u32> {
        let v = x.valor(clave)?;
        match v.trim().parse::<u32>() {
            Ok(n) if (min..=max).contains(&n) => Some(n),
            _ => {
                avisos.push(format!("{}: {:?} no vale (de {} a {})", clave, v, min, max));
                None
            }
        }
    };
    if let Some(n) = numero("duracion", 100, 5000, &mut avisos) {
        c.duracion = n;
    }
    if let Some(n) = numero("pase", 0, 24 * 60, &mut avisos) {
        c.pase = n;
    }
    if let Some(n) = numero("cpu", 1, 50, &mut avisos) {
        c.cpu = n * 10;
    }
    if let Some(v) = x.valor("carpeta") {
        let v = v.trim();
        if nombre_vale(v) && !v.starts_with("animado:") {
            c.carpeta = camino(v);
        } else {
            avisos.push(format!("carpeta: {:?} no vale", v));
        }
    }
    if let Some(v) = x.valor("efecto") {
        let v = v.trim().to_ascii_lowercase();
        if v == "ninguno" || v.is_empty() {
            c.efecto = None;
        } else if ["luciernagas", "nieve", "aurora"].contains(&v.as_str()) {
            c.efecto = Some(v);
        } else {
            avisos.push(format!("efecto: {:?} no es ninguno", v));
        }
    }
    (c, avisos)
}

impl Config {
    /// El fondo de la pantalla `salida` en el modo `oscuro`.
    pub fn elegir(&self, salida: &str, oscuro: bool) -> &str {
        let propia = self.pantallas.iter().find(|p| p.0.eq_ignore_ascii_case(salida));
        let (claro_p, oscuro_p) = propia.map_or((None, None), |p| (p.1.as_deref(), p.2.as_deref()));
        if oscuro {
            oscuro_p.or(claro_p).or(self.oscuro.as_deref()).unwrap_or(&self.fondo)
        } else {
            claro_p.unwrap_or(&self.fondo)
        }
    }

    /// Pone el fondo de `salida` (o de todas, con `None`) para `oscuro` (o para claro).
    pub fn poner(&mut self, salida: Option<&str>, oscuro: bool, fondo: &str) {
        let f = camino(fondo);
        match salida {
            None if oscuro => self.oscuro = Some(f),
            None => self.fondo = f,
            Some(s) => {
                let i = match self.pantallas.iter().position(|p| p.0.eq_ignore_ascii_case(s)) {
                    Some(i) => i,
                    None => {
                        self.pantallas.push((s.to_string(), None, None));
                        self.pantallas.len() - 1
                    }
                };
                if oscuro { self.pantallas[i].2 = Some(f) } else { self.pantallas[i].1 = Some(f) }
            }
        }
    }

    /// Pone el fondo de `salida` (o de todas) para los dos modos: el claro, y el oscuro deja de ser otro.
    pub fn poner_los_dos(&mut self, salida: Option<&str>, fondo: &str) {
        self.poner(salida, false, fondo);
        match salida {
            None => self.oscuro = None,
            Some(s) => {
                if let Some(p) = self.pantallas.iter_mut().find(|p| p.0.eq_ignore_ascii_case(s)) {
                    p.2 = None;
                }
            }
        }
    }

    /// El texto, sobre el que había (`antes`: sus comentarios y lo que no es del fondo se quedan).
    pub fn a_texto(&self, antes: Option<&[u8]>) -> Vec<u8> {
        let mut x = antes.and_then(|b| Ajustes::leer(b).ok()).unwrap_or_else(|| {
            let mut x = Ajustes::nuevo();
            x.comentar(CABECERA);
            x
        });
        let _ = x.poner("fondo", &self.fondo);
        match &self.oscuro {
            Some(o) => {
                let _ = x.poner("oscuro", o);
            }
            None => {
                x.quitar("oscuro");
            }
        }
        for (s, claro, oscuro) in &self.pantallas {
            for (pre, v) in [("fondo-", claro), ("oscuro-", oscuro)] {
                let clave = format!("{}{}", pre, s);
                match v {
                    Some(v) => {
                        let _ = x.poner(&clave, v);
                    }
                    None => {
                        x.quitar(&clave);
                    }
                }
            }
        }
        let _ = x.poner("ajuste", self.ajuste.nombre());
        let _ = x.poner("color", &format!("{:06x}", self.color & 0xff_ffff));
        let _ = x.poner("transicion", &self.transicion);
        let _ = x.poner("duracion", &self.duracion.to_string());
        let _ = x.poner("pase", &self.pase.to_string());
        let _ = x.poner("carpeta", &self.carpeta);
        let _ = x.poner("efecto", self.efecto.as_deref().unwrap_or("ninguno"));
        let _ = x.poner("cpu", &(self.cpu / 10).max(1).to_string());
        x.texto().into_bytes()
    }
}
