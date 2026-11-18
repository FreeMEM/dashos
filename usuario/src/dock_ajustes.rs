//! S27: los ajustes del dock y de los avisos en pantalla (OSD), en texto (S36, regla 1): `ENV:Escritorio/dock` y
//! `ENV:Escritorio/osd` (en uso), `ENVARC:` (guardados), en el formato de `kit::idioma::texto` (`clave = valor`).
//!
//! El modelo es el de DankMaterialShell v2.1.2 (`Common/settings/DockConfig.js`: `create`, `normalize`, `pick`;
//! `Common/settings/SettingsSpec.js`: los `osd*`). Copyright (c) 2025 Avenge Media LLC, licencia MIT
//! (`docs/licencias/DankMaterialShell-MIT.txt`). Lo de serie es lo del escritorio del usuario en shiva (su
//! `dockConfigs` y sus `osd*`, decisión del usuario del 2026-11-16: "el mismo aspecto que mi escritorio"); lo que su
//! fichero no dice, lo de serie de DMS.
//!
//! Cada clave lleva al lado la de DMS. Lo que no vale (un número fuera de rango, una palabra que no es ninguna) se
//! recorta o se queda con lo de serie, como `normalize`, y se dice (`Problema`), sin romper el resto. Una clave que no se
//! conoce también se dice, y no cuenta.
//!
//! Sin `alloc` de DashOS ni nada de `dashos`: entra por pares (clave, valor) y sale por pares; quien lee el texto es
//! `kit::idioma::texto`. Se prueba en el PC con `rustc --edition 2024 --test usuario/src/dock_ajustes.rs`.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Los ficheros, dentro de `ENV:` y `ENVARC:`.
pub const FICHERO_DOCK: &str = "Escritorio/dock";
pub const FICHERO_OSD: &str = "Escritorio/osd";
/// Programas fijados, como mucho, y lo que mide cada nombre.
pub const MAX_FIJADOS: usize = 64;
pub const MAX_NOMBRE: usize = 64;
/// Monitores nombrados, como mucho.
pub const MAX_MONITORES: usize = 8;

/// Los bordes (`SettingsData.Position`: 0 arriba, 1 abajo, 2 izquierda, 3 derecha).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Borde {
    Arriba,
    Abajo,
    Izquierda,
    Derecha,
}

impl Borde {
    pub fn vertical(self) -> bool {
        matches!(self, Borde::Izquierda | Borde::Derecha)
    }
    /// Si está al final del eje (abajo o a la derecha): el indicador va por fuera, hacia ese borde.
    pub fn al_final(self) -> bool {
        matches!(self, Borde::Abajo | Borde::Derecha)
    }
    fn nombre(self) -> &'static str {
        match self {
            Borde::Arriba => "arriba",
            Borde::Abajo => "abajo",
            Borde::Izquierda => "izquierda",
            Borde::Derecha => "derecha",
        }
    }
    fn de(t: &str) -> Option<Borde> {
        [Borde::Arriba, Borde::Abajo, Borde::Izquierda, Borde::Derecha].into_iter().find(|b| b.nombre().eq_ignore_ascii_case(t))
    }
}

/// `mode`: "compact" (del ancho de lo que lleva) o "taskbar" (de lado a lado).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modo {
    Compacto,
    Barra,
}

/// `taskbarAlign`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alinear {
    Inicio,
    Centro,
    Fin,
}

/// `indicatorStyle` ("circle", "line"), y "ninguno" (`hideIndicators` de la barra de DMS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indicador {
    Circulo,
    Linea,
    Ninguno,
}

/// `borderColor`: un papel del tema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PapelBorde {
    Texto,
    Primario,
    Secundario,
}

/// `middleClickAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClicCentral {
    Cerrar,
    Abrir,
}

/// `magnificationProfile`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfilAumento {
    Parabolico,
    Coseno,
    Gauss,
}

/// Los ajustes de un dock (`dockConfigs[0]` de DMS). Las medidas, en píxeles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dock {
    /// `enabled`.
    pub activo: bool,
    /// `position`.
    pub posicion: Borde,
    /// `screenPreferences`: vacío es "todos" ("all").
    pub monitores: Vec<String>,
    /// `showOnLastDisplay`: con un solo monitor, sale en él aunque no sea el nombrado.
    pub ultimo_monitor: bool,
    pub modo: Modo,
    pub alinear: Alinear,
    /// `iconSize` (16 a 96), `spacing` (0 a 128), `itemSpacing` (0 a 32), `margin` (0 a 128), `bottomGap` (-128 a 128).
    pub iconos: i32,
    pub relleno: i32,
    pub separacion: i32,
    pub margen: i32,
    pub hueco: i32,
    /// `transparency` (0 a 1 en DMS; aquí, en %: lo que se ve del fondo del dock).
    pub opacidad: u32,
    /// `Theme.windowRadius` del dock (en shiva, el de niri: `niriLayoutRadiusOverride` 3; en DMS sin niri, 16).
    pub radio: i32,
    /// `autoHide` y `smartAutoHide` (esconderse solo si una ventana lo tapa).
    pub autoocultar: bool,
    pub ocultar_con_ventanas: bool,
    /// `groupByApp` y `separatePinnedAndRunningApps`.
    pub agrupar: bool,
    pub separar_fijados: bool,
    pub indicador: Indicador,
    /// `borderEnabled`, `borderColor`, `borderOpacity` (%), `borderThickness`.
    pub borde: bool,
    pub color_borde: PapelBorde,
    pub opacidad_borde: u32,
    pub grosor_borde: i32,
    /// `launcherEnabled`.
    pub lanzador: bool,
    /// `maxVisibleApps`, `maxVisibleRunningApps` (0 a 100; 0, sin límite) y `showOverflowBadge`.
    pub max_fijados: u32,
    pub max_abiertos: u32,
    pub insignia_desbordado: bool,
    /// `magnification`, `magnificationScale` (105 a 160 %), `magnificationProfile`.
    pub aumento: bool,
    pub escala_aumento: u32,
    pub perfil_aumento: PerfilAumento,
    /// `magnificationExpand`: los que crecen apartan a los vecinos y el dock se alarga.
    pub aumento_expandir: bool,
    pub clic_central: ClicCentral,
    /// `showOnFullscreen`.
    pub pantalla_completa: bool,
    /// Los programas fijados (`SessionData.pinnedApps` en DMS), por su nombre en `Programas/`.
    pub fijados: Vec<String>,
}

impl Dock {
    /// Lo de serie: el `dockConfigs` de shiva (lo que no dice, lo de serie de DMS).
    pub fn de_serie() -> Dock {
        Dock {
            activo: true,
            posicion: Borde::Abajo,
            monitores: Vec::new(),
            ultimo_monitor: true,
            modo: Modo::Compacto,
            alinear: Alinear::Centro,
            iconos: 40,
            relleno: 4,
            separacion: 4,
            margen: 0,
            hueco: 0,
            opacidad: 66,
            radio: 3,
            autoocultar: true,
            ocultar_con_ventanas: false,
            agrupar: false,
            separar_fijados: false,
            indicador: Indicador::Circulo,
            borde: false,
            color_borde: PapelBorde::Texto,
            opacidad_borde: 100,
            grosor_borde: 1,
            lanzador: true,
            max_fijados: 0,
            max_abiertos: 0,
            insignia_desbordado: true,
            aumento: false,
            escala_aumento: 130,
            perfil_aumento: PerfilAumento::Parabolico,
            aumento_expandir: false,
            clic_central: ClicCentral::Cerrar,
            pantalla_completa: false,
            fijados: Vec::new(),
        }
    }

    /// `DockConfig.effectiveThickness`: lo que mide de grueso (el icono y el relleno de los dos lados).
    pub fn grosor(&self) -> i32 {
        self.iconos + self.relleno * 2
    }

    /// Si está fijado `programa`.
    pub fn fijado(&self, programa: &str) -> bool {
        self.fijados.iter().any(|f| f == programa)
    }
}

/// Las ocho posiciones de un OSD (`SettingsData.Position`: 0 arriba a la derecha... ver `alinear`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosOsd {
    /// `Top` (0): arriba a la derecha.
    ArribaDerecha,
    /// `Bottom` (1): abajo a la izquierda.
    AbajoIzquierda,
    /// `Left` (2): arriba a la izquierda.
    ArribaIzquierda,
    /// `Right` (3): abajo a la derecha.
    AbajoDerecha,
    /// `TopCenter` (4).
    ArribaCentro,
    /// `BottomCenter` (5, lo de serie de DMS).
    AbajoCentro,
    /// `LeftCenter` (6): a la izquierda, en vertical.
    IzquierdaCentro,
    /// `RightCenter` (7): a la derecha, en vertical.
    DerechaCentro,
}

pub const POSICIONES_OSD: [(PosOsd, &str); 8] = [
    (PosOsd::ArribaDerecha, "arriba-derecha"),
    (PosOsd::AbajoIzquierda, "abajo-izquierda"),
    (PosOsd::ArribaIzquierda, "arriba-izquierda"),
    (PosOsd::AbajoDerecha, "abajo-derecha"),
    (PosOsd::ArribaCentro, "arriba-centro"),
    (PosOsd::AbajoCentro, "abajo-centro"),
    (PosOsd::IzquierdaCentro, "izquierda-centro"),
    (PosOsd::DerechaCentro, "derecha-centro"),
];

impl PosOsd {
    /// `DOSD.alignX` y `alignY`: -1 al principio, 0 en medio, 1 al final.
    pub fn alinear(self) -> (i32, i32) {
        match self {
            PosOsd::ArribaDerecha => (1, -1),
            PosOsd::AbajoIzquierda => (-1, 1),
            PosOsd::ArribaIzquierda => (-1, -1),
            PosOsd::AbajoDerecha => (1, 1),
            PosOsd::ArribaCentro => (0, -1),
            PosOsd::AbajoCentro => (0, 1),
            PosOsd::IzquierdaCentro => (-1, 0),
            PosOsd::DerechaCentro => (1, 0),
        }
    }
    /// `isVerticalLayout`.
    pub fn vertical(self) -> bool {
        matches!(self, PosOsd::IzquierdaCentro | PosOsd::DerechaCentro)
    }
    pub fn nombre(self) -> &'static str {
        POSICIONES_OSD.iter().find(|p| p.0 == self).unwrap().1
    }
    fn de(t: &str) -> Option<PosOsd> {
        POSICIONES_OSD.iter().find(|p| p.1.eq_ignore_ascii_case(t)).map(|p| p.0)
    }
}

/// Los ajustes de los OSD (`osd*` de DMS).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Osd {
    /// `osdPosition` (shiva: 4, arriba en el centro; de serie en DMS: abajo en el centro).
    pub posicion: PosOsd,
    /// `osdAlwaysShowValue` (shiva: sí).
    pub siempre_valor: bool,
    /// `osdVolumeEnabled`, `osdBrightnessEnabled`, `osdCapsLockEnabled`, `osdWorkspaceEnabled` (este, no de serie).
    pub volumen: bool,
    pub brillo: bool,
    pub mayusculas: bool,
    pub espacio: bool,
    /// `radiusStrength` (0 a 100; shiva 38, DMS 50): lo redondo de la píldora (50 es del todo).
    pub redondez: u32,
}

impl Osd {
    pub fn de_serie() -> Osd {
        Osd { posicion: PosOsd::ArribaCentro, siempre_valor: true, volumen: true, brillo: true, mayusculas: true, espacio: false, redondez: 38 }
    }
}

/// Lo que se dice de un ajuste que no se pudo usar como está (con su clave).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problema {
    /// No es ninguna clave de este fichero.
    Desconocida(String),
    /// No vale: se queda lo de serie.
    NoVale(String),
    /// Un número fuera de su rango: se recorta.
    Recortado(String),
}

impl Problema {
    pub fn texto(&self) -> String {
        match self {
            Problema::Desconocida(c) => format!("{}: no es ningún ajuste", c),
            Problema::NoVale(c) => format!("{}: no vale; queda lo de serie", c),
            Problema::Recortado(c) => format!("{}: fuera de su rango; recortado", c),
        }
    }
}

fn si_no(v: &str) -> Option<bool> {
    match v.to_lowercase().as_str() {
        "sí" | "si" | "1" => Some(true),
        "no" | "0" => Some(false),
        _ => None,
    }
}

pub fn texto_si_no(b: bool) -> &'static str {
    if b { "sí" } else { "no" }
}

/// Un número entero (con signo), recortado a `min..=max`.
fn numero(clave: &str, v: &str, min: i32, max: i32, problemas: &mut Vec<Problema>) -> Option<i32> {
    match v.trim().parse::<i64>() {
        Ok(n) => {
            let r = n.clamp(min as i64, max as i64) as i32;
            if r as i64 != n {
                problemas.push(Problema::Recortado(clave.to_string()));
            }
            Some(r)
        }
        Err(_) => {
            problemas.push(Problema::NoVale(clave.to_string()));
            None
        }
    }
}

/// Un nombre de programa o de monitor que vale: sin barras, sin control, sin vacíos ni puntos solos.
pub fn nombre_valido(n: &str) -> bool {
    !n.is_empty() && n.len() <= MAX_NOMBRE && n != "." && n != ".." && !n.contains('/') && !n.contains(':') && !n.contains(',') && !n.chars().any(char::is_control)
}

/// Una lista separada por comas: los que valen, sin repetir, hasta `max`. `false` si quedó fuera alguno.
fn lista(v: &str, max: usize) -> (Vec<String>, bool) {
    let mut fuera = false;
    let mut l: Vec<String> = Vec::new();
    for parte in v.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if !nombre_valido(parte) || l.len() >= max {
            fuera = true;
            continue;
        }
        if !l.iter().any(|x| x == parte) {
            l.push(parte.to_string());
        }
    }
    (l, fuera)
}

macro_rules! palabra {
    ($clave:expr, $v:expr, $p:expr, $( $txt:literal => $val:expr ),+ $(,)?) => {{
        match $v.to_lowercase().as_str() {
            $( $txt => Some($val), )+
            _ => {
                $p.push(Problema::NoVale($clave.to_string()));
                None
            }
        }
    }};
}

impl Dock {
    /// Los ajustes de los pares (clave, valor) del fichero, sobre lo de serie. Las claves no distinguen mayúsculas.
    pub fn de_pares<'a>(pares: impl IntoIterator<Item = (&'a str, &'a str)>) -> (Dock, Vec<Problema>) {
        let mut d = Dock::de_serie();
        let mut p = Vec::new();
        for (clave, v) in pares {
            let c = clave.to_lowercase();
            let mal = |p: &mut Vec<Problema>| p.push(Problema::NoVale(clave.to_string()));
            match c.as_str() {
                "activo" => match si_no(v) { Some(b) => d.activo = b, None => mal(&mut p) },
                "posicion" => match Borde::de(v) { Some(b) => d.posicion = b, None => mal(&mut p) },
                "monitores" => {
                    if v.eq_ignore_ascii_case("todos") {
                        d.monitores.clear();
                    } else {
                        let (l, fuera) = lista(v, MAX_MONITORES);
                        if fuera || l.is_empty() {
                            mal(&mut p);
                        }
                        if !l.is_empty() {
                            d.monitores = l;
                        }
                    }
                }
                "ultimo_monitor" => match si_no(v) { Some(b) => d.ultimo_monitor = b, None => mal(&mut p) },
                "modo" => if let Some(m) = palabra!(clave, v, p, "compacto" => Modo::Compacto, "barra" => Modo::Barra) { d.modo = m },
                "alinear" => if let Some(a) = palabra!(clave, v, p, "inicio" => Alinear::Inicio, "centro" => Alinear::Centro, "fin" => Alinear::Fin) { d.alinear = a },
                "iconos" => if let Some(n) = numero(clave, v, 16, 96, &mut p) { d.iconos = n },
                "relleno" => if let Some(n) = numero(clave, v, 0, 128, &mut p) { d.relleno = n },
                "separacion" => if let Some(n) = numero(clave, v, 0, 32, &mut p) { d.separacion = n },
                "margen" => if let Some(n) = numero(clave, v, 0, 128, &mut p) { d.margen = n },
                "hueco" => if let Some(n) = numero(clave, v, -128, 128, &mut p) { d.hueco = n },
                "opacidad" => if let Some(n) = numero(clave, v, 0, 100, &mut p) { d.opacidad = n as u32 },
                "radio" => if let Some(n) = numero(clave, v, 0, 48, &mut p) { d.radio = n },
                "autoocultar" => match si_no(v) { Some(b) => d.autoocultar = b, None => mal(&mut p) },
                "ocultar_con_ventanas" => match si_no(v) { Some(b) => d.ocultar_con_ventanas = b, None => mal(&mut p) },
                "agrupar" => match si_no(v) { Some(b) => d.agrupar = b, None => mal(&mut p) },
                "separar_fijados" => match si_no(v) { Some(b) => d.separar_fijados = b, None => mal(&mut p) },
                "indicador" => if let Some(i) = palabra!(clave, v, p, "circulo" => Indicador::Circulo, "círculo" => Indicador::Circulo, "linea" => Indicador::Linea, "línea" => Indicador::Linea, "ninguno" => Indicador::Ninguno) { d.indicador = i },
                "borde" => match si_no(v) { Some(b) => d.borde = b, None => mal(&mut p) },
                "color_borde" => if let Some(c) = palabra!(clave, v, p, "texto" => PapelBorde::Texto, "primario" => PapelBorde::Primario, "secundario" => PapelBorde::Secundario) { d.color_borde = c },
                "opacidad_borde" => if let Some(n) = numero(clave, v, 0, 100, &mut p) { d.opacidad_borde = n as u32 },
                "grosor_borde" => if let Some(n) = numero(clave, v, 1, 8, &mut p) { d.grosor_borde = n },
                "lanzador" => match si_no(v) { Some(b) => d.lanzador = b, None => mal(&mut p) },
                "max_fijados" => if let Some(n) = numero(clave, v, 0, 100, &mut p) { d.max_fijados = n as u32 },
                "max_abiertos" => if let Some(n) = numero(clave, v, 0, 100, &mut p) { d.max_abiertos = n as u32 },
                "insignia_desbordado" => match si_no(v) { Some(b) => d.insignia_desbordado = b, None => mal(&mut p) },
                "aumento" => match si_no(v) { Some(b) => d.aumento = b, None => mal(&mut p) },
                "escala_aumento" => if let Some(n) = numero(clave, v, 105, 160, &mut p) { d.escala_aumento = n as u32 },
                "perfil_aumento" => if let Some(x) = palabra!(clave, v, p, "parabolico" => PerfilAumento::Parabolico, "parabólico" => PerfilAumento::Parabolico, "coseno" => PerfilAumento::Coseno, "gauss" => PerfilAumento::Gauss) { d.perfil_aumento = x },
                "aumento_expandir" => match si_no(v) { Some(b) => d.aumento_expandir = b, None => mal(&mut p) },
                "clic_central" => if let Some(x) = palabra!(clave, v, p, "cerrar" => ClicCentral::Cerrar, "abrir" => ClicCentral::Abrir) { d.clic_central = x },
                "pantalla_completa" => match si_no(v) { Some(b) => d.pantalla_completa = b, None => mal(&mut p) },
                "fijados" => {
                    let (l, fuera) = lista(v, MAX_FIJADOS);
                    if fuera {
                        mal(&mut p);
                    }
                    d.fijados = l;
                }
                _ => p.push(Problema::Desconocida(clave.to_string())),
            }
        }
        (d, p)
    }

    /// Los pares que se escriben (todos, en el orden en que salen en Ajustes).
    pub fn a_pares(&self) -> Vec<(&'static str, String)> {
        let s = |b: bool| texto_si_no(b).to_string();
        alloc::vec![
            ("activo", s(self.activo)),
            ("posicion", self.posicion.nombre().to_string()),
            ("monitores", if self.monitores.is_empty() { "todos".to_string() } else { self.monitores.join(", ") }),
            ("ultimo_monitor", s(self.ultimo_monitor)),
            ("modo", match self.modo { Modo::Compacto => "compacto", Modo::Barra => "barra" }.to_string()),
            ("alinear", match self.alinear { Alinear::Inicio => "inicio", Alinear::Centro => "centro", Alinear::Fin => "fin" }.to_string()),
            ("iconos", self.iconos.to_string()),
            ("relleno", self.relleno.to_string()),
            ("separacion", self.separacion.to_string()),
            ("margen", self.margen.to_string()),
            ("hueco", self.hueco.to_string()),
            ("opacidad", self.opacidad.to_string()),
            ("radio", self.radio.to_string()),
            ("autoocultar", s(self.autoocultar)),
            ("ocultar_con_ventanas", s(self.ocultar_con_ventanas)),
            ("agrupar", s(self.agrupar)),
            ("separar_fijados", s(self.separar_fijados)),
            ("indicador", match self.indicador { Indicador::Circulo => "circulo", Indicador::Linea => "linea", Indicador::Ninguno => "ninguno" }.to_string()),
            ("borde", s(self.borde)),
            ("color_borde", match self.color_borde { PapelBorde::Texto => "texto", PapelBorde::Primario => "primario", PapelBorde::Secundario => "secundario" }.to_string()),
            ("opacidad_borde", self.opacidad_borde.to_string()),
            ("grosor_borde", self.grosor_borde.to_string()),
            ("lanzador", s(self.lanzador)),
            ("max_fijados", self.max_fijados.to_string()),
            ("max_abiertos", self.max_abiertos.to_string()),
            ("insignia_desbordado", s(self.insignia_desbordado)),
            ("aumento", s(self.aumento)),
            ("escala_aumento", self.escala_aumento.to_string()),
            ("perfil_aumento", match self.perfil_aumento { PerfilAumento::Parabolico => "parabolico", PerfilAumento::Coseno => "coseno", PerfilAumento::Gauss => "gauss" }.to_string()),
            ("aumento_expandir", s(self.aumento_expandir)),
            ("clic_central", match self.clic_central { ClicCentral::Cerrar => "cerrar", ClicCentral::Abrir => "abrir" }.to_string()),
            ("pantalla_completa", s(self.pantalla_completa)),
            ("fijados", self.fijados.join(", ")),
        ]
    }
}

/// La cabecera del fichero del dock (comentarios), con la clave de DMS de cada ajuste.
pub const CABECERA_DOCK: &str = "El dock (Ajustes > Dock). Este fichero manda; editarlo a mano vale igual que la ventana.\n\
Como el dock de DankMaterialShell (dockConfigs); entre paréntesis, su clave allí.\n\
activo (enabled): sí o no. posicion (position): abajo, arriba, izquierda o derecha.\n\
monitores (screenPreferences): todos, o nombres separados por comas. ultimo_monitor (showOnLastDisplay): con un solo monitor, sale en él.\n\
modo (mode): compacto o barra. alinear (taskbarAlign): inicio, centro o fin.\n\
iconos (iconSize) 16-96, relleno (spacing) 0-128, separacion (itemSpacing) 0-32, margen (margin) 0-128, hueco (bottomGap) -128-128, en píxeles.\n\
opacidad (transparency): 0-100 %. radio: el de las esquinas.\n\
autoocultar (autoHide), ocultar_con_ventanas (smartAutoHide), agrupar (groupByApp), separar_fijados (separatePinnedAndRunningApps): sí o no.\n\
indicador (indicatorStyle): circulo, linea o ninguno. borde, color_borde (texto, primario, secundario), opacidad_borde, grosor_borde.\n\
lanzador (launcherEnabled). max_fijados y max_abiertos (maxVisibleApps, maxVisibleRunningApps; 0 sin límite). insignia_desbordado.\n\
aumento (magnification), escala_aumento (105-160), perfil_aumento (parabolico, coseno, gauss), aumento_expandir (magnificationExpand). clic_central: cerrar o abrir. pantalla_completa.\n\
fijados: los programas fijados, por su nombre en Programas, separados por comas.";

impl Osd {
    pub fn de_pares<'a>(pares: impl IntoIterator<Item = (&'a str, &'a str)>) -> (Osd, Vec<Problema>) {
        let mut o = Osd::de_serie();
        let mut p = Vec::new();
        for (clave, v) in pares {
            let mal = |p: &mut Vec<Problema>| p.push(Problema::NoVale(clave.to_string()));
            match clave.to_lowercase().as_str() {
                "posicion" => match PosOsd::de(v) { Some(x) => o.posicion = x, None => mal(&mut p) },
                "siempre_valor" => match si_no(v) { Some(b) => o.siempre_valor = b, None => mal(&mut p) },
                "volumen" => match si_no(v) { Some(b) => o.volumen = b, None => mal(&mut p) },
                "brillo" => match si_no(v) { Some(b) => o.brillo = b, None => mal(&mut p) },
                "mayusculas" => match si_no(v) { Some(b) => o.mayusculas = b, None => mal(&mut p) },
                "espacio" => match si_no(v) { Some(b) => o.espacio = b, None => mal(&mut p) },
                "redondez" => if let Some(n) = numero(clave, v, 0, 100, &mut p) { o.redondez = n as u32 },
                _ => p.push(Problema::Desconocida(clave.to_string())),
            }
        }
        (o, p)
    }

    pub fn a_pares(&self) -> Vec<(&'static str, String)> {
        let s = |b: bool| texto_si_no(b).to_string();
        alloc::vec![
            ("posicion", self.posicion.nombre().to_string()),
            ("siempre_valor", s(self.siempre_valor)),
            ("volumen", s(self.volumen)),
            ("brillo", s(self.brillo)),
            ("mayusculas", s(self.mayusculas)),
            ("espacio", s(self.espacio)),
            ("redondez", self.redondez.to_string()),
        ]
    }
}

pub const CABECERA_OSD: &str = "Los avisos en pantalla (OSD) del volumen, el brillo, Bloq Mayús y el espacio de trabajo (Ajustes > Dock).\n\
Este fichero manda. Como los OSD de DankMaterialShell; entre paréntesis, su clave allí.\n\
posicion (osdPosition): arriba-centro, abajo-centro, arriba-izquierda, arriba-derecha, abajo-izquierda, abajo-derecha, izquierda-centro o derecha-centro.\n\
siempre_valor (osdAlwaysShowValue), volumen, brillo, mayusculas, espacio (osd*Enabled): sí o no.\n\
redondez (radiusStrength): 0-100; 50 es una píldora.";

/// `DockConfig.pick` con un solo dock: en qué monitores sale, de los que hay (por nombre). Nombrado y presente, en
/// ese; "todos", en todos; si no casa ninguno y solo hay uno, en él si `ultimo_monitor`. Si no, en ninguno.
pub fn monitores_del_dock(d: &Dock, hay: &[&str]) -> Vec<usize> {
    if !d.activo || hay.is_empty() {
        return Vec::new();
    }
    if d.monitores.is_empty() {
        return (0..hay.len()).collect();
    }
    let nombrados: Vec<usize> = (0..hay.len()).filter(|&i| d.monitores.iter().any(|m| m == hay[i])).collect();
    if !nombrados.is_empty() {
        return nombrados;
    }
    if hay.len() == 1 && d.ultimo_monitor {
        return alloc::vec![0];
    }
    Vec::new()
}

#[cfg(test)]
#[path = "ajustes_texto.rs"]
#[allow(dead_code)]
mod texto;

#[cfg(test)]
mod pruebas {
    use super::*;

    fn leer(t: &str) -> (Dock, Vec<Problema>) {
        let a = texto::Ajustes::leer(t.as_bytes()).expect("texto");
        let pares: Vec<(String, String)> = a.todos().map(|(c, v)| (c.to_string(), v.to_string())).collect();
        Dock::de_pares(pares.iter().map(|(c, v)| (c.as_str(), v.as_str())))
    }

    #[test]
    fn de_serie_es_lo_de_shiva() {
        let d = Dock::de_serie();
        // dockConfigs de shiva: position 1, iconSize 40, spacing 4, itemSpacing 4, margin 0, transparency 0.66,
        // autoHide, sin agrupar, indicador circle, lanzador.
        assert_eq!((d.posicion, d.iconos, d.relleno, d.separacion, d.margen, d.opacidad), (Borde::Abajo, 40, 4, 4, 0, 66));
        assert!(d.autoocultar && !d.agrupar && d.lanzador && d.activo);
        assert_eq!(d.grosor(), 48);
        let o = Osd::de_serie();
        assert_eq!(o.posicion, PosOsd::ArribaCentro);
        assert!(o.siempre_valor && o.volumen && o.mayusculas && !o.espacio);
    }

    #[test]
    fn ida_y_vuelta() {
        let mut d = Dock::de_serie();
        d.fijados = alloc::vec!["Shell".into(), "hola".into()];
        d.posicion = Borde::Izquierda;
        d.monitores = alloc::vec!["HDMI-A-3".into()];
        let pares = d.a_pares();
        let (d2, p) = Dock::de_pares(pares.iter().map(|(c, v)| (*c, v.as_str())));
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(d, d2);
        let o = Osd { posicion: PosOsd::DerechaCentro, espacio: true, ..Osd::de_serie() };
        let pares = o.a_pares();
        assert_eq!(Osd::de_pares(pares.iter().map(|(c, v)| (*c, v.as_str()))), (o, Vec::new()));
        for (pos, n) in POSICIONES_OSD {
            assert_eq!(PosOsd::de(n), Some(pos));
        }
    }

    #[test]
    fn el_texto_con_comentarios() {
        let (d, p) = leer("# El dock\nposicion = izquierda\nICONOS = 56\n; otro comentario\nfijados = Shell, hola , , Shell\nautoocultar = no\n");
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(d.posicion, Borde::Izquierda);
        assert_eq!(d.iconos, 56);
        assert!(!d.autoocultar);
        assert_eq!(d.fijados, alloc::vec![String::from("Shell"), String::from("hola")]);
    }

    #[test]
    fn hostil_fichero_malo() {
        // Números fuera de rango, palabras que no son nada, claves inventadas, nombres con barras: lo demás vale.
        let (d, p) = leer("iconos = 100000\nrelleno = -5\nposicion = diagonal\nautoocultar = quizá\ninventada = 1\nfijados = ../../etc, ok, a/b, C:x\nhueco = 99999999999999999999\nopacidad = treinta\nmonitores = \nradio = 7\n");
        assert_eq!(d.iconos, 96);
        assert_eq!(d.relleno, 0);
        assert_eq!(d.posicion, Borde::Abajo);
        assert!(d.autoocultar);
        assert_eq!(d.fijados, alloc::vec![String::from("ok")]);
        assert_eq!(d.hueco, 0, "un número que no cabe ni en i64 no vale");
        assert_eq!(d.opacidad, 66);
        assert_eq!(d.radio, 7);
        assert!(d.monitores.is_empty());
        for esperado in [
            Problema::Recortado("iconos".into()),
            Problema::Recortado("relleno".into()),
            Problema::NoVale("posicion".into()),
            Problema::NoVale("autoocultar".into()),
            Problema::Desconocida("inventada".into()),
            Problema::NoVale("fijados".into()),
            Problema::NoVale("hueco".into()),
            Problema::NoVale("opacidad".into()),
            Problema::NoVale("monitores".into()),
        ] {
            assert!(p.contains(&esperado), "falta {esperado:?} en {p:?}");
        }
    }

    #[test]
    fn hostil_mil_fijados_y_nombres_largos() {
        let mut v = String::new();
        for i in 0..1000 {
            v.push_str(&format!("p{},", i));
        }
        v.push_str(&"x".repeat(5000));
        let (d, p) = Dock::de_pares([("fijados", v.as_str())]);
        assert_eq!(d.fijados.len(), MAX_FIJADOS);
        assert!(p.contains(&Problema::NoVale("fijados".into())));
        let (d, _) = Dock::de_pares([("fijados", "a\u{7}b, \u{0}, ñandú")]);
        assert_eq!(d.fijados, alloc::vec![String::from("ñandú")]);
    }

    #[test]
    fn hostil_bytes_al_azar() {
        // 2000 ficheros de bytes al azar: o no son texto, o se leen sin romper nada.
        let mut x: u64 = 0x1234_5678_9abc_def0;
        for _ in 0..2000 {
            let mut b = Vec::new();
            for _ in 0..(x % 300) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                b.push(if x % 5 == 0 { b'=' } else if x % 7 == 0 { b'\n' } else { (x >> 24) as u8 });
            }
            if let Ok(a) = texto::Ajustes::leer(&b) {
                let pares: Vec<(String, String)> = a.todos().map(|(c, v)| (c.to_string(), v.to_string())).collect();
                let (d, _) = Dock::de_pares(pares.iter().map(|(c, v)| (c.as_str(), v.as_str())));
                assert!((16..=96).contains(&d.iconos));
                let (o, _) = Osd::de_pares(pares.iter().map(|(c, v)| (c.as_str(), v.as_str())));
                assert!(o.redondez <= 100);
            }
        }
    }

    #[test]
    fn monitores() {
        let mut d = Dock::de_serie();
        assert_eq!(monitores_del_dock(&d, &["A", "B"]), alloc::vec![0, 1]);
        d.monitores = alloc::vec!["B".into()];
        assert_eq!(monitores_del_dock(&d, &["A", "B"]), alloc::vec![1]);
        // Un monitor que no existe: con uno solo, sale en él (showOnLastDisplay); con dos, en ninguno.
        d.monitores = alloc::vec!["NO-EXISTE".into()];
        assert_eq!(monitores_del_dock(&d, &["A"]), alloc::vec![0]);
        assert!(monitores_del_dock(&d, &["A", "B"]).is_empty());
        d.ultimo_monitor = false;
        assert!(monitores_del_dock(&d, &["A"]).is_empty());
        assert!(monitores_del_dock(&d, &[]).is_empty());
        d.activo = false;
        d.monitores.clear();
        assert!(monitores_del_dock(&d, &["A"]).is_empty());
    }
}
