//! Las barras del escritorio, configurables como en DMS (`docs/bitacoras/dms-barra.md`, S27 sección 0).
//!
//! Portado de DankMaterialShell, MIT, Copyright (c) 2025 Avenge Media LLC (`docs/licencias/DankMaterialShell-MIT.txt`):
//! el modelo de `barConfigs` (`Common/settings/SettingsSpec.js`), las opciones de cada widget
//! (`Common/settings/BarWidgetDefaults.js`) y el catálogo de widgets (`Common/BarWidgetCatalog.qml`), de la v2.1.2,
//! llevados a Rust y al fichero de texto de DashOS.
//!
//! El fichero (S36, regla 1: el texto manda) es `ENV:Escritorio/barra` (en uso) y `ENVARC:Escritorio/barra` (guardado),
//! en el formato de `idioma::texto` (`clave = valor`). Lo escriben la ventana (Ajustes > Barra), la orden `ImportarDMS`
//! y una persona con un editor; lo lee la barra, que cambia en marcha. Un ejemplo:
//!
//! ```text
//! barras = principal
//! radios = 50
//! principal-posicion = arriba
//! principal-monitores = todos
//! principal-izquierda = lanzador, espacios(numeros), ventana, menus
//! principal-centro = reloj
//! principal-derecha = cpu, memoria, -disco, avisos, estado(porcentaje)
//! ```
//!
//! - Las claves globales van sueltas; las de cada barra, con su nombre delante (`principal-`). Una clave que falta vale
//!   lo de serie (el de DMS), así que un fichero con una sola línea ya vale.
//! - Cada zona es una lista separada por comas. Cada widget es su nombre (el de `REGISTRO`); con `-` delante, está pero
//!   apagado; entre paréntesis, sus opciones (`clave=valor`, o `clave` sola para "sí"; un valor con espacios, entre
//!   comillas).
//! - Un widget que no está en el registro, o una opción que no se conoce, se guardan tal cual (no se pierde lo que
//!   escribió nadie), no se usan y se dicen.
//!
//! Sin nada de DashOS: se prueba con `rustc --edition 2024 --test usuario/src/barra_config.rs` (con `make prueba`).

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[cfg(test)]
#[path = "ajustes_texto.rs"]
mod texto;
#[cfg(not(test))]
use kit::idioma::texto;
#[path = "json_mini.rs"]
mod json;

use json::Json;
use texto::Ajustes;

/// El fichero, dentro de `ENV:` y de `ENVARC:`.
pub const FICHERO: &str = "Escritorio/barra";
/// Como en DMS: hasta cuatro barras.
pub const MAX_BARRAS: usize = 4;
/// Lo más de widgets en una zona (lo que cabe en una línea del fichero).
pub const MAX_WIDGETS: usize = 24;
/// Lo más de opciones de un widget.
pub const MAX_OPCIONES: usize = 8;
/// Lo más que mide el nombre de una barra (va delante de cada clave suya) y el de un widget.
pub const MAX_ID: usize = 16;
pub const MAX_NOMBRE: usize = 64;
/// Lo más de monitores nombrados en una barra.
pub const MAX_MONITORES: usize = 8;

/// Lo que va encima de un fichero nuevo.
pub const CABECERA: &str = "Las barras del escritorio (Ajustes > Barra; la orden `ImportarDMS`). Este fichero manda.\n\
El modelo es el de DankMaterialShell (barConfigs). Cada zona: widgets separados por comas; -nombre, apagado;\n\
nombre(opción=valor ...), con sus opciones. Lo que falta vale lo de serie.";

// ---------------------------------------------------------------------------------------------------------------
// El registro de widgets (el catálogo de DMS).

/// El tipo de una opción.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    SiNo,
    /// Un número entero entre estos dos.
    Numero(i32, i32),
    /// Uno de estos.
    Uno(&'static [&'static str]),
}

/// Una opción de un widget: su clave en el fichero, la de DMS, el nombre que se ve, su tipo y lo de serie (el de DMS).
#[derive(Clone, Copy, Debug)]
pub struct Opcion {
    pub clave: &'static str,
    pub dms: &'static str,
    pub nombre: &'static str,
    pub tipo: Tipo,
    pub serie: &'static str,
    /// Para `Uno`: cómo se llaman en DMS los valores (en el orden de `Tipo::Uno`).
    pub dms_valores: &'static [&'static str],
}

const fn si_no(clave: &'static str, dms: &'static str, nombre: &'static str, serie: bool) -> Opcion {
    Opcion { clave, dms, nombre, tipo: Tipo::SiNo, serie: if serie { "sí" } else { "no" }, dms_valores: &[] }
}

/// Un widget que conoce DashOS.
#[derive(Clone, Copy, Debug)]
pub struct Ficha {
    /// El nombre en el fichero.
    pub id: &'static str,
    /// El de DMS (`BarWidgetCatalog`), o "" si es solo de DashOS.
    pub dms: &'static str,
    pub nombre: &'static str,
    pub explica: &'static str,
    /// El icono de Material Symbols (el de DMS), para cuando el kit los tenga.
    pub icono: &'static str,
    /// La zona en la que lo pone DMS al añadirlo (0 izquierda, 1 centro, 2 derecha).
    pub zona: usize,
    /// Si aún no hay de dónde sacar lo que enseña: el porqué. Se guarda en el fichero y en Ajustes, pero no se ve en la
    /// barra hasta que lo haya.
    pub falta: Option<&'static str>,
    pub opciones: &'static [Opcion],
    /// Solo uno por barra.
    pub uno: bool,
}

const fn ficha(id: &'static str, dms: &'static str, nombre: &'static str, explica: &'static str, icono: &'static str, zona: usize) -> Ficha {
    Ficha { id, dms, nombre, explica, icono, zona, falta: None, opciones: &[], uno: false }
}

const MINIMO: Opcion = si_no("minimo", "minimumWidth", "Ancho fijo (no baila al cambiar el número)", true);

/// Los widgets, en el orden del catálogo de DMS (y los de DashOS donde tocan).
pub const REGISTRO: &[Ficha] = &[
    Ficha {
        opciones: &[Opcion { clave: "logo", dms: "launcherLogoMode", nombre: "Logo", tipo: Tipo::Uno(&["apps", "sistema"]), serie: "apps", dms_valores: &["apps", "os"] }],
        ..ficha("lanzador", "launcherButton", "Lanzador", "Abre el lanzador de programas", "apps", 0)
    },
    Ficha {
        opciones: &[
            Opcion { clave: "estilo", dms: "workspaceIndicatorStyle", nombre: "Estilo", tipo: Tipo::Uno(&["pildoras", "puntos", "lineas", "tarjetas"]), serie: "pildoras", dms_valores: &["pills", "dots", "lines", "cards"] },
            si_no("numeros", "showWorkspaceIndex", "Con su número", false),
            si_no("relleno", "showWorkspacePadding", "Enseñar siempre unos cuantos", true),
            Opcion { clave: "cuantos", dms: "workspacePaddingCount", nombre: "Cuántos como poco", tipo: Tipo::Numero(1, 9), serie: "3", dms_valores: &[] },
            si_no("ocupados", "showOccupiedWorkspacesOnly", "Solo los que tienen ventanas", false),
            si_no("compacto", "workspaceIndicatorCompact", "Compacto", false),
            si_no("borde", "workspaceFocusedBorderEnabled", "Borde en el que se ve", false),
        ],
        ..ficha("espacios", "workspaceSwitcher", "Espacios de trabajo", "El que se ve y los demás; un clic cambia", "view_module", 0)
    },
    Ficha {
        opciones: &[si_no("compacto", "focusedWindowCompactMode", "Compacto", false)],
        ..ficha("ventana", "focusedWindow", "Ventana con el teclado", "El título de la ventana en la que se escribe", "window", 0)
    },
    Ficha { uno: true, ..ficha("menus", "", "Menús del programa", "Los menús del programa activo, si van en la barra (Ajustes > Escritorio)", "menu", 0) },
    Ficha { falta: Some("la barra aún no sabe qué programas hay abiertos"), ..ficha("programas", "runningApps", "Programas abiertos", "Los programas abiertos, con el que tiene el teclado", "apps", 0) },
    Ficha { falta: Some("el dock va aparte"), ..ficha("dock", "appsDock", "Dock en la barra", "Programas fijados y abiertos", "dock_to_bottom", 0) },
    Ficha { falta: Some("la isla de DMS no está hecha"), uno: true, ..ficha("isla", "island", "Isla", "Lo que pasa, creciendo desde la barra", "blur_on", 1) },
    Ficha {
        opciones: &[si_no("compacto", "clockCompactMode", "Solo la hora", false), Opcion { clave: "orden", dms: "clockDateOrder", nombre: "Orden", tipo: Tipo::Uno(&["hora", "fecha"]), serie: "hora", dms_valores: &["timeFirst", "dateFirst"] }],
        ..ficha("reloj", "clock", "Reloj", "La hora y la fecha", "schedule", 1)
    },
    Ficha { falta: Some("no hay servicio del tiempo"), ..ficha("tiempo", "weather", "El tiempo", "El tiempo que hace", "wb_sunny", 1) },
    Ficha { falta: Some("no hay servicio de música"), ..ficha("musica", "music", "Música", "Lo que suena y sus botones", "music_note", 1) },
    Ficha { falta: Some("no hay historial del portapapeles"), ..ficha("portapapeles", "clipboard", "Portapapeles", "El historial de lo copiado", "content_paste", 2) },
    Ficha { opciones: &[MINIMO], ..ficha("cpu", "cpuUsage", "CPU", "Cuánto trabaja el procesador; un clic, los procesos", "memory", 2) },
    Ficha { opciones: &[MINIMO, si_no("gb", "showInGb", "En GB", false)], ..ficha("memoria", "memUsage", "Memoria", "La memoria en uso; un clic, los procesos", "developer_board", 2) },
    Ficha { opciones: &[MINIMO], ..ficha("disco", "diskUsage", "Disco", "Lo ocupado del disco del sistema", "storage", 2) },
    Ficha { falta: Some("no hay sensor de temperatura"), opciones: &[MINIMO], ..ficha("temperatura", "cpuTemp", "Temperatura", "La del procesador", "device_thermostat", 2) },
    Ficha { falta: Some("no hay sensor de temperatura"), opciones: &[MINIMO], ..ficha("temperatura_gpu", "gpuTemp", "Temperatura de la GPU", "La de la tarjeta gráfica", "auto_awesome_mosaic", 2) },
    Ficha { falta: Some("no hay bandeja de programas"), ..ficha("bandeja", "systemTray", "Bandeja", "Los iconos de los programas en segundo plano", "notifications", 2) },
    Ficha { falta: Some("no se sabe aún quién usa el micrófono o la cámara"), ..ficha("privacidad", "privacyIndicator", "Privacidad", "Si el micrófono, la cámara o la pantalla están en uso", "privacy_tip", 2) },
    Ficha {
        opciones: &[si_no("red", "showNetworkIcon", "Icono de la red", true), si_no("sonido", "showAudioIcon", "Icono del sonido", true), si_no("porcentaje", "showAudioPercent", "Volumen en %", false)],
        ..ficha("estado", "controlCenterButton", "Centro de control", "La red y el sonido; un clic abre el centro de control", "settings", 2)
    },
    ficha("avisos", "notificationButton", "Avisos", "La campana, con un punto si hay sin leer", "notifications", 2),
    Ficha { falta: Some("no hay batería ni servicio de energía"), ..ficha("bateria", "battery", "Batería", "La carga y la energía", "battery_std", 2) },
    Ficha { falta: Some("no hay VPN"), ..ficha("vpn", "vpn", "VPN", "La VPN y conectar", "vpn_lock", 2) },
    Ficha { falta: Some("no hay quien mantenga despierta la pantalla"), ..ficha("despierto", "idleInhibitor", "Mantener despierto", "Que la pantalla no se apague", "local_cafe", 2) },
    Ficha { falta: Some("la barra no sabe si las mayúsculas están puestas"), ..ficha("mayusculas", "capsLockIndicator", "Mayúsculas", "Si están puestas", "shift_lock", 2) },
    Ficha {
        opciones: &[Opcion { clave: "tam", dms: "size", nombre: "Tamaño", tipo: Tipo::Numero(1, 400), serie: "20", dms_valores: &[] }],
        ..ficha("espaciador", "spacer", "Espacio", "Un hueco del tamaño que se diga", "more_horiz", 2)
    },
    ficha("separador", "separator", "Separador", "Una raya entre widgets", "remove", 2),
    Ficha { falta: Some("la red no da aún la velocidad"), ..ficha("red_velocidad", "network_speed_monitor", "Velocidad de la red", "Lo que baja y lo que sube", "network_check", 2) },
    Ficha { falta: Some("hay una sola distribución del teclado"), ..ficha("teclado", "keyboard_layout_name", "Teclado", "La distribución del teclado", "keyboard", 2) },
    Ficha { falta: Some("no hay bloc de notas"), ..ficha("notas", "notepadButton", "Notas", "El bloc de notas", "assignment", 2) },
    Ficha { falta: Some("no hay cuentagotas"), ..ficha("color", "colorPicker", "Cuentagotas", "Coger un color de la pantalla", "palette", 2) },
    Ficha { falta: Some("Paq aún no avisa de actualizaciones"), ..ficha("actualizaciones", "systemUpdate", "Actualizaciones", "Si hay actualizaciones", "update", 2) },
    Ficha { falta: Some("no hay menú de apagado"), ..ficha("apagado", "powerMenuButton", "Apagar", "El menú de apagar y reiniciar", "power_settings_new", 2) },
];

/// La ficha de `id` (el nombre de DashOS, sin distinguir mayúsculas).
pub fn ficha_de(id: &str) -> Option<&'static Ficha> {
    REGISTRO.iter().find(|f| f.id.eq_ignore_ascii_case(id))
}

/// La ficha del widget de DMS `dms`.
pub fn ficha_de_dms(dms: &str) -> Option<&'static Ficha> {
    REGISTRO.iter().find(|f| !f.dms.is_empty() && f.dms == dms)
}

// ---------------------------------------------------------------------------------------------------------------
// El modelo.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Posicion {
    Arriba,
    Abajo,
    Izquierda,
    Derecha,
}

impl Posicion {
    pub const TODAS: [Posicion; 4] = [Posicion::Arriba, Posicion::Abajo, Posicion::Izquierda, Posicion::Derecha];

    pub fn nombre(self) -> &'static str {
        match self {
            Posicion::Arriba => "arriba",
            Posicion::Abajo => "abajo",
            Posicion::Izquierda => "izquierda",
            Posicion::Derecha => "derecha",
        }
    }

    pub fn de(t: &str) -> Option<Posicion> {
        Posicion::TODAS.into_iter().find(|p| p.nombre().eq_ignore_ascii_case(t.trim()))
    }

    /// De lado (las zonas van de arriba abajo).
    pub fn vertical(self) -> bool {
        matches!(self, Posicion::Izquierda | Posicion::Derecha)
    }
}

/// Los colores por su papel en el tema (los nombres de DMS: `surfaceText`, `primary`...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Papel {
    Texto,
    TextoSuave,
    Primario,
    Secundario,
    Terciario,
    Contorno,
    Error,
}

const PAPELES: [(Papel, &str, &str); 7] = [
    (Papel::Texto, "texto", "surfaceText"),
    (Papel::TextoSuave, "texto_suave", "surfaceVariantText"),
    (Papel::Primario, "primario", "primary"),
    (Papel::Secundario, "secundario", "secondary"),
    (Papel::Terciario, "terciario", "tertiary"),
    (Papel::Contorno, "contorno", "outline"),
    (Papel::Error, "error", "error"),
];

impl Papel {
    pub fn nombre(self) -> &'static str {
        PAPELES.iter().find(|p| p.0 == self).map_or("texto", |p| p.1)
    }
    pub fn de(t: &str) -> Option<Papel> {
        PAPELES.iter().find(|p| p.1.eq_ignore_ascii_case(t.trim())).map(|p| p.0)
    }
    fn de_dms(t: &str) -> Option<Papel> {
        PAPELES.iter().find(|p| p.2 == t).map(|p| p.0)
    }
}

/// Las tres zonas: izquierda, centro y derecha (en una barra de lado, arriba, en medio y abajo).
pub const ZONAS: [&str; 3] = ["izquierda", "centro", "derecha"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Widget {
    pub id: String,
    pub activo: bool,
    pub opciones: Vec<(String, String)>,
}

impl Widget {
    pub fn nuevo(id: &str) -> Widget {
        Widget { id: id.to_string(), activo: true, opciones: Vec::new() }
    }

    pub fn ficha(&self) -> Option<&'static Ficha> {
        ficha_de(&self.id)
    }

    /// El valor de la opción `clave`: el puesto o, si no, el de serie de su ficha.
    pub fn opcion(&self, clave: &str) -> Option<&str> {
        if let Some((_, v)) = self.opciones.iter().find(|(k, _)| k.eq_ignore_ascii_case(clave)) {
            return Some(v);
        }
        self.ficha()?.opciones.iter().find(|o| o.clave == clave).map(|o| o.serie)
    }

    /// Una opción de sí o no.
    pub fn si(&self, clave: &str) -> bool {
        self.opcion(clave).and_then(si_o_no).unwrap_or(false)
    }

    /// Una opción de número (dentro de su rango; mal escrita, la de serie).
    pub fn numero(&self, clave: &str) -> i32 {
        let op = self.ficha().and_then(|f| f.opciones.iter().find(|o| o.clave == clave));
        let serie = op.and_then(|o| o.serie.parse().ok()).unwrap_or(0);
        let v = self.opcion(clave).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(serie);
        match op.map(|o| o.tipo) {
            Some(Tipo::Numero(a, b)) => v.clamp(a, b),
            _ => v,
        }
    }

    /// Pone una opción (o la quita si vale lo de serie).
    pub fn poner(&mut self, clave: &str, valor: &str) {
        let serie = self.ficha().and_then(|f| f.opciones.iter().find(|o| o.clave == clave)).map(|o| o.serie);
        self.opciones.retain(|(k, _)| !k.eq_ignore_ascii_case(clave));
        if serie != Some(valor) {
            self.opciones.push((clave.to_string(), valor.to_string()));
        }
    }

    /// Si se dibuja: encendido, conocido y con dato.
    pub fn se_ve(&self) -> bool {
        self.activo && self.ficha().is_some_and(|f| f.falta.is_none())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Monitores {
    Todos,
    /// Por nombre (`DASHOS-1`, `DASHOS-2`...; los de DMS, como `HDMI-A-3`, no existen en DashOS).
    Estos(Vec<String>),
}

/// Una barra (un `barConfig` de DMS).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Barra {
    pub id: String,
    pub nombre: String,
    pub activa: bool,
    pub visible: bool,
    pub posicion: Posicion,
    pub monitores: Monitores,
    /// `showOnLastDisplay`: si ninguno de sus monitores está y solo hay uno, sale en ese.
    pub ultimo_monitor: bool,
    /// `spacing`: lo que la separa de los bordes de la pantalla.
    pub espaciado: i32,
    /// `innerPadding`: el relleno de dentro, que da el grosor de la barra y de sus widgets.
    pub relleno: i32,
    /// `bottomGap`: un hueco de más entre la barra y las ventanas.
    pub hueco: i32,
    /// `transparency` y `widgetTransparency`, en %: lo opaca que es la barra y sus widgets.
    pub opacidad: u32,
    pub opacidad_widgets: u32,
    pub sin_fondo: bool,
    pub esquinas_rectas: bool,
    /// `attachToScreenEdge`: pegada al borde, sin hueco y sin redondear por ese lado.
    pub pegada: bool,
    /// `borderEnabled` y lo suyo: color, opacidad (%) y grosor.
    pub borde: bool,
    pub borde_color: Papel,
    pub borde_opacidad: u32,
    pub borde_grosor: i32,
    /// `widgetOutlineEnabled` y lo suyo.
    pub contorno: bool,
    pub contorno_color: Papel,
    pub contorno_opacidad: u32,
    pub contorno_grosor: i32,
    /// `fontScale` e `iconScale`, en %.
    pub letra: u32,
    pub iconos: u32,
    /// `widgetStyle`: píldoras (cada widget en la suya) o plano (sin píldora, con el radio pequeño al pulsar).
    pub plano: bool,
    /// `autoHide` y `autoHideDelay` (ms).
    pub autoocultar: bool,
    pub retardo: u32,
    /// `shadowIntensity` (0, sin sombra propia).
    pub sombra: u32,
    pub zonas: [Vec<Widget>; 3],
}

fn ws(ids: &[&str]) -> Vec<Widget> {
    ids.iter().map(|i| Widget::nuevo(i)).collect()
}

impl Barra {
    /// Una barra nueva con lo de serie de DMS (`SettingsSpec.barConfigs.def`), con los menús del programa tras la
    /// ventana (lo único de DashOS). El relleno, 0 y no 2: así mide 36 + 4 de hueco, los 40 de la barra de siempre (las
    /// ventanas no se mueven); con el 2 de DMS serían 42.
    pub fn nueva(id: &str) -> Barra {
        Barra {
            id: id.to_string(),
            nombre: String::from("Barra principal"),
            activa: true,
            visible: true,
            posicion: Posicion::Arriba,
            monitores: Monitores::Todos,
            ultimo_monitor: true,
            espaciado: 4,
            relleno: 0,
            hueco: 0,
            opacidad: 88,
            opacidad_widgets: 100,
            sin_fondo: false,
            esquinas_rectas: false,
            pegada: false,
            borde: false,
            borde_color: Papel::Texto,
            borde_opacidad: 100,
            borde_grosor: 1,
            contorno: false,
            contorno_color: Papel::Primario,
            contorno_opacidad: 100,
            contorno_grosor: 1,
            letra: 100,
            iconos: 100,
            plano: false,
            autoocultar: false,
            retardo: 250,
            sombra: 0,
            zonas: [
                ws(&["lanzador", "espacios", "ventana", "menus"]),
                ws(&["musica", "reloj", "tiempo"]),
                ws(&["bandeja", "portapapeles", "cpu", "memoria", "avisos", "bateria", "estado"]),
            ],
        }
    }

    /// Lo que mide la barra de grueso (DMS, `Theme.barThickness`, con `barHeight` 48): el mayor de lo que piden sus
    /// widgets más el relleno y de 36 + relleno, en número par.
    pub fn grosor(&self) -> i32 {
        let r = self.relleno.clamp(0, MAX_RELLENO);
        par((self.grosor_widgets() + r + 4).max(48 - 4 - (8 - r)))
    }

    /// Lo que miden los widgets de grueso (DMS, `Theme.barWidgetThickness`): 26 + 0,6 del relleno, en número par.
    pub fn grosor_widgets(&self) -> i32 {
        let r = self.relleno.clamp(0, MAX_RELLENO);
        par((260 + r * 6 + 5) / 10).max(20)
    }

    /// La zona que aparta de las ventanas (DMS, `surfaceExclusiveZone`): grosor, hueco al borde (sin él si va pegada) y
    /// el hueco de abajo. Con autoocultar o sin verse, nada.
    pub fn zona_exclusiva(&self) -> i32 {
        if !self.visible || self.autoocultar {
            return 0;
        }
        self.grosor() + if self.pegada { 0 } else { self.espaciado.clamp(0, MAX_ESPACIADO) } + self.hueco.clamp(0, MAX_HUECO)
    }

    /// Lo que la separa del borde (sin él si va pegada).
    pub fn hueco_borde(&self) -> i32 {
        if self.pegada { 0 } else { self.espaciado.clamp(0, MAX_ESPACIADO) }
    }

    /// Si sale en el monitor `nombre`, siendo `total` los que hay (`showOnLastDisplay`: si ninguno de los suyos está y
    /// solo hay uno, en ese).
    pub fn en_monitor(&self, nombre: &str, existentes: &[&str]) -> bool {
        if !self.activa {
            return false;
        }
        match &self.monitores {
            Monitores::Todos => true,
            Monitores::Estos(v) => {
                if v.iter().any(|m| m.eq_ignore_ascii_case(nombre)) {
                    return true;
                }
                let alguno = v.iter().any(|m| existentes.iter().any(|e| e.eq_ignore_ascii_case(m)));
                self.ultimo_monitor && !alguno && existentes.len() == 1
            }
        }
    }

    /// Los monitores nombrados que no existen (para decirlo).
    pub fn monitores_que_faltan(&self, existentes: &[&str]) -> Vec<String> {
        match &self.monitores {
            Monitores::Todos => Vec::new(),
            Monitores::Estos(v) => v.iter().filter(|m| !existentes.iter().any(|e| e.eq_ignore_ascii_case(m))).cloned().collect(),
        }
    }
}

const MAX_RELLENO: i32 = 24;
const MAX_ESPACIADO: i32 = 32;
const MAX_HUECO: i32 = 64;

fn par(v: i32) -> i32 {
    // Como `Theme.snapEven`: al par más cercano (los .5, hacia arriba).
    2 * ((v + 1) / 2)
}

/// Todas las barras y lo común a ellas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub barras: Vec<Barra>,
    /// `radiusStrength` de DMS (0 a 100; 50, los radios de Material): escala todos los radios.
    pub radios: u32,
    /// El radio de las ventanas del compositor (con niri, `niriLayoutRadiusOverride`), que es también el de las
    /// esquinas de la barra. -1: el de `radios` (16 en Material).
    pub radio_ventanas: i32,
    /// `interfaceStyle`: expresivo (píldoras, se deforman al pulsar) o clásico (el mismo radio en todo).
    pub clasico: bool,
    /// `clockDateFormat` (con los códigos de Qt: d, dd, ddd, dddd, M, MM, MMM, MMMM, yy, yyyy). Vacío: "ddd d MMM".
    pub formato_fecha: String,
}

impl Config {
    pub fn de_serie() -> Config {
        Config { barras: alloc::vec![Barra::nueva("principal")], radios: 50, radio_ventanas: -1, clasico: false, formato_fecha: String::new() }
    }

    pub fn barra(&self, id: &str) -> Option<&Barra> {
        self.barras.iter().find(|b| b.id.eq_ignore_ascii_case(id))
    }

    /// La escala de los radios (DMS, `Shape.scaleForStrength`): `radios` / 50, en milésimas.
    fn escala_mil(&self) -> i32 {
        self.radios.min(100) as i32 * 20
    }

    /// Un radio de Material (los "tokens" de DMS: xs 4, s 8, m 12, l 16, xl 28) escalado por `radios`.
    pub fn radio(&self, base: i32) -> i32 {
        (base * self.escala_mil() + 500) / 1000
    }

    /// El de las esquinas de la barra (DMS, `Theme.windowRadius`): el de las ventanas o el `l` (16) escalado.
    pub fn radio_barra(&self, b: &Barra) -> i32 {
        if b.esquinas_rectas {
            return 0;
        }
        if self.radio_ventanas >= 0 { self.radio_ventanas.min(64) } else { self.radio(16) }
    }

    /// El de las píldoras de los widgets de grueso `g` (DMS, `BarMetrics.pillRadius`): en expresivo, la mitad del grueso
    /// por la escala (sin pasar de la mitad); en clásico, el `l`; si es más fina que 28, el `m`.
    pub fn radio_pildora(&self, b: &Barra, g: i32) -> i32 {
        if b.plano {
            return self.radio(8).min(g / 2);
        }
        if g < 28 {
            return self.radio(12).min(g / 2);
        }
        if self.clasico { self.radio(16).min(g / 2) } else { (g / 2 * self.escala_mil().min(1000) + 500) / 1000 }
    }

    /// El de una píldora pulsada (DMS: se deforma hasta el `s`, 8).
    pub fn radio_pulsada(&self, b: &Barra, g: i32) -> i32 {
        if self.clasico { self.radio_pildora(b, g) } else { self.radio(8).min(self.radio_pildora(b, g)) }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El fichero de texto.

/// Sí o no, como los escribe una persona.
pub fn si_o_no(t: &str) -> Option<bool> {
    match t.trim().to_lowercase().as_str() {
        "sí" | "si" | "s" | "yes" | "true" | "1" | "on" => Some(true),
        "no" | "n" | "false" | "0" | "off" => Some(false),
        _ => None,
    }
}

fn texto_si(b: bool) -> &'static str {
    if b { "sí" } else { "no" }
}

/// Si `id` vale como nombre de barra o de widget (letras y cifras ASCII y `_`; va dentro de las claves).
pub fn id_valido(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Parte una zona (`lanzador, espacios(numeros cuantos=5), -reloj`) en sus widgets. Lo que no se entiende se dice.
pub fn leer_zona(valor: &str, avisos: &mut Vec<String>, donde: &str) -> Vec<Widget> {
    let mut v = Vec::new();
    for trozo in partir_fuera(valor, ',') {
        let t = trozo.trim();
        if t.is_empty() {
            continue;
        }
        if v.len() >= MAX_WIDGETS {
            avisos.push(format!("{}: más de {} widgets; los de después no se usan", donde, MAX_WIDGETS));
            break;
        }
        let (activo, t) = match t.strip_prefix('-') {
            Some(r) => (false, r.trim_start()),
            None => (true, t),
        };
        let (id, resto) = match t.find('(') {
            Some(i) => (t[..i].trim(), Some(&t[i + 1..])),
            None => (t, None),
        };
        if !id_valido(id) {
            avisos.push(format!("{}: \"{}\" no es un nombre de widget (letras, cifras y _)", donde, trozo.trim()));
            continue;
        }
        let mut w = Widget { id: id.to_string(), activo, opciones: Vec::new() };
        if let Some(resto) = resto {
            let Some(dentro) = resto.trim_end().strip_suffix(')') else {
                avisos.push(format!("{}: a {} le falta el paréntesis de cierre", donde, id));
                continue;
            };
            for op in partir_fuera(dentro, ' ') {
                let op = op.trim();
                if op.is_empty() {
                    continue;
                }
                if w.opciones.len() >= MAX_OPCIONES {
                    avisos.push(format!("{}: {} lleva más de {} opciones; las de después no se usan", donde, id, MAX_OPCIONES));
                    break;
                }
                let (k, val) = match op.split_once('=') {
                    Some((k, val)) => (k.trim(), val.trim().trim_matches('"')),
                    None => (op, "sí"),
                };
                if !id_valido(k) || val.contains(['"', '(', ')', ',']) {
                    avisos.push(format!("{}: {}: la opción \"{}\" no se entiende", donde, id, op));
                    continue;
                }
                w.opciones.retain(|(x, _)| !x.eq_ignore_ascii_case(k));
                w.opciones.push((k.to_string(), val.to_string()));
            }
        }
        // Lo que no se conoce, se guarda y se dice.
        match w.ficha() {
            None => avisos.push(format!("{}: el widget \"{}\" no existe en DashOS: se guarda, pero no se enseña", donde, id)),
            Some(f) => {
                for (k, val) in &w.opciones {
                    match f.opciones.iter().find(|o| o.clave.eq_ignore_ascii_case(k)) {
                        None => avisos.push(format!("{}: {} no tiene la opción \"{}\": se guarda, pero no se usa", donde, id, k)),
                        Some(o) if !valor_vale(o.tipo, val) => avisos.push(format!("{}: {}: \"{}\" no vale para {}; vale lo de serie ({})", donde, id, val, o.clave, o.serie)),
                        _ => {}
                    }
                }
            }
        }
        v.push(w);
    }
    v
}

fn valor_vale(t: Tipo, v: &str) -> bool {
    match t {
        Tipo::SiNo => si_o_no(v).is_some(),
        Tipo::Numero(a, b) => v.trim().parse::<i32>().is_ok_and(|n| (a..=b).contains(&n)),
        Tipo::Uno(vs) => vs.iter().any(|x| x.eq_ignore_ascii_case(v.trim())),
    }
}

/// Parte `t` por `sep`, sin partir dentro de paréntesis ni de comillas.
fn partir_fuera(t: &str, sep: char) -> Vec<&str> {
    let mut v = Vec::new();
    let (mut hondo, mut comillas, mut desde) = (0i32, false, 0);
    for (i, c) in t.char_indices() {
        match c {
            '"' => comillas = !comillas,
            '(' if !comillas => hondo += 1,
            ')' if !comillas => hondo = (hondo - 1).max(0),
            c if c == sep && hondo == 0 && !comillas => {
                v.push(&t[desde..i]);
                desde = i + c.len_utf8();
            }
            _ => {}
        }
    }
    v.push(&t[desde..]);
    v
}

/// Una zona en texto.
pub fn escribir_zona(ws: &[Widget]) -> String {
    let mut s = String::new();
    for (k, w) in ws.iter().enumerate() {
        if k > 0 {
            s.push_str(", ");
        }
        if !w.activo {
            s.push('-');
        }
        s.push_str(&w.id);
        if !w.opciones.is_empty() {
            s.push('(');
            for (j, (o, v)) in w.opciones.iter().enumerate() {
                if j > 0 {
                    s.push(' ');
                }
                s.push_str(o);
                if si_o_no(v) != Some(true) || !matches!(w.ficha().and_then(|f| f.opciones.iter().find(|x| x.clave == o.as_str())).map(|x| x.tipo), Some(Tipo::SiNo)) {
                    s.push('=');
                    if v.is_empty() || v.contains(' ') {
                        s.push('"');
                        s.push_str(v);
                        s.push('"');
                    } else {
                        s.push_str(v);
                    }
                }
            }
            s.push(')');
        }
    }
    s
}

/// Las claves de cada barra (tras `<id>-`).
const CLAVES_BARRA: &[&str] = &[
    "nombre", "activa", "visible", "posicion", "monitores", "ultimo_monitor", "espaciado", "relleno", "hueco", "opacidad", "opacidad_widgets",
    "sin_fondo", "esquinas_rectas", "pegada", "borde", "borde_color", "borde_opacidad", "borde_grosor", "contorno", "contorno_color",
    "contorno_opacidad", "contorno_grosor", "letra", "iconos", "estilo", "autoocultar", "retardo", "sombra", "izquierda", "centro", "derecha",
];
const CLAVES_GLOBALES: &[&str] = &["barras", "radios", "radio_ventanas", "estilo", "formato_fecha"];

impl Config {
    /// Lee el fichero. Nunca falla: lo que no vale se dice y vale lo de serie.
    pub fn leer(bytes: &[u8]) -> (Config, Vec<String>) {
        let mut avisos = Vec::new();
        let a = match Ajustes::leer(bytes) {
            Ok(a) => a,
            Err(e) => {
                avisos.push(format!("{} no se lee ({}): lo de serie", FICHERO, if e == texto::Error::NoEsTexto { "no es texto" } else { "demasiado grande" }));
                return (Config::de_serie(), avisos);
            }
        };
        for (linea, p) in a.problemas() {
            avisos.push(format!("línea {}: {}", linea, p.texto()));
        }
        let mut c = Config::de_serie();
        let num = |clave: &str, min: i64, max: i64, serie: i64, avisos: &mut Vec<String>| -> i64 {
            match a.valor(clave) {
                None => serie,
                Some(v) => match v.trim().parse::<i64>() {
                    Ok(n) if (min..=max).contains(&n) => n,
                    Ok(n) => {
                        avisos.push(format!("{} = {}: fuera de {} a {}; vale {}", clave, v, min, max, n.clamp(min, max)));
                        n.clamp(min, max)
                    }
                    Err(_) => {
                        avisos.push(format!("{} = {}: no es un número; vale {}", clave, v, serie));
                        serie
                    }
                },
            }
        };
        let si = |clave: &str, serie: bool, avisos: &mut Vec<String>| -> bool {
            match a.valor(clave) {
                None => serie,
                Some(v) => si_o_no(v).unwrap_or_else(|| {
                    avisos.push(format!("{} = {}: ni sí ni no; vale {}", clave, v, texto_si(serie)));
                    serie
                }),
            }
        };
        c.radios = num("radios", 0, 100, 50, &mut avisos) as u32;
        c.radio_ventanas = num("radio_ventanas", -1, 64, -1, &mut avisos) as i32;
        c.clasico = match a.valor("estilo").map(|v| v.trim().to_lowercase()) {
            None => false,
            Some(v) if v == "clasico" || v == "clásico" => true,
            Some(v) if v == "expresivo" => false,
            Some(v) => {
                avisos.push(format!("estilo = {}: ni expresivo ni clasico; vale expresivo", v));
                false
            }
        };
        c.formato_fecha = a.valor("formato_fecha").unwrap_or("").to_string();
        // Las barras, en el orden de `barras`.
        let ids: Vec<String> = match a.valor("barras") {
            None => alloc::vec![String::from("principal")],
            Some(v) => {
                let mut ids: Vec<String> = Vec::new();
                for id in v.split(',').map(str::trim).filter(|x| !x.is_empty()) {
                    if !id_valido(id) {
                        avisos.push(format!("barras: \"{}\" no vale como nombre (letras, cifras y _, hasta {})", id, MAX_ID));
                    } else if ids.iter().any(|x| x.eq_ignore_ascii_case(id)) {
                        avisos.push(format!("barras: \"{}\" está dos veces; vale una", id));
                    } else if ids.len() >= MAX_BARRAS {
                        avisos.push(format!("barras: más de {}; \"{}\" y las de después no se usan", MAX_BARRAS, id));
                        break;
                    } else {
                        ids.push(id.to_string());
                    }
                }
                ids
            }
        };
        c.barras.clear();
        for id in ids {
            let mut b = Barra::nueva(&id);
            let k = |campo: &str| format!("{}-{}", id, campo);
            if let Some(v) = a.valor(&k("nombre")) {
                b.nombre = v.chars().take(MAX_NOMBRE).collect();
            }
            b.activa = si(&k("activa"), b.activa, &mut avisos);
            b.visible = si(&k("visible"), b.visible, &mut avisos);
            if let Some(v) = a.valor(&k("posicion")) {
                match Posicion::de(v) {
                    Some(p) => b.posicion = p,
                    None => avisos.push(format!("{} = {}: arriba, abajo, izquierda o derecha; vale arriba", k("posicion"), v)),
                }
            }
            if let Some(v) = a.valor(&k("monitores")) {
                b.monitores = if v.trim().eq_ignore_ascii_case("todos") || v.trim().is_empty() {
                    Monitores::Todos
                } else {
                    let ms: Vec<String> = v.split(',').map(str::trim).filter(|x| !x.is_empty()).take(MAX_MONITORES).map(String::from).collect();
                    Monitores::Estos(ms)
                };
            }
            b.ultimo_monitor = si(&k("ultimo_monitor"), b.ultimo_monitor, &mut avisos);
            b.espaciado = num(&k("espaciado"), 0, MAX_ESPACIADO as i64, b.espaciado as i64, &mut avisos) as i32;
            b.relleno = num(&k("relleno"), 0, MAX_RELLENO as i64, b.relleno as i64, &mut avisos) as i32;
            b.hueco = num(&k("hueco"), 0, MAX_HUECO as i64, b.hueco as i64, &mut avisos) as i32;
            b.opacidad = num(&k("opacidad"), 0, 100, b.opacidad as i64, &mut avisos) as u32;
            b.opacidad_widgets = num(&k("opacidad_widgets"), 0, 100, b.opacidad_widgets as i64, &mut avisos) as u32;
            b.sin_fondo = si(&k("sin_fondo"), b.sin_fondo, &mut avisos);
            b.esquinas_rectas = si(&k("esquinas_rectas"), b.esquinas_rectas, &mut avisos);
            b.pegada = si(&k("pegada"), b.pegada, &mut avisos);
            b.borde = si(&k("borde"), b.borde, &mut avisos);
            b.contorno = si(&k("contorno"), b.contorno, &mut avisos);
            for (campo, destino) in [("borde_color", &mut b.borde_color), ("contorno_color", &mut b.contorno_color)] {
                if let Some(v) = a.valor(&k(campo)) {
                    match Papel::de(v) {
                        Some(p) => *destino = p,
                        None => avisos.push(format!("{} = {}: no es un papel del tema (texto, primario, contorno...)", k(campo), v)),
                    }
                }
            }
            b.borde_opacidad = num(&k("borde_opacidad"), 0, 100, b.borde_opacidad as i64, &mut avisos) as u32;
            b.borde_grosor = num(&k("borde_grosor"), 1, 8, b.borde_grosor as i64, &mut avisos) as i32;
            b.contorno_opacidad = num(&k("contorno_opacidad"), 0, 100, b.contorno_opacidad as i64, &mut avisos) as u32;
            b.contorno_grosor = num(&k("contorno_grosor"), 1, 8, b.contorno_grosor as i64, &mut avisos) as i32;
            b.letra = num(&k("letra"), 50, 200, b.letra as i64, &mut avisos) as u32;
            b.iconos = num(&k("iconos"), 50, 200, b.iconos as i64, &mut avisos) as u32;
            if let Some(v) = a.valor(&k("estilo")) {
                match v.trim() {
                    "pildoras" | "píldoras" => b.plano = false,
                    "plano" => b.plano = true,
                    _ => avisos.push(format!("{} = {}: pildoras o plano", k("estilo"), v)),
                }
            }
            b.autoocultar = si(&k("autoocultar"), b.autoocultar, &mut avisos);
            b.retardo = num(&k("retardo"), 0, 10_000, b.retardo as i64, &mut avisos) as u32;
            b.sombra = num(&k("sombra"), 0, 100, b.sombra as i64, &mut avisos) as u32;
            for (z, nombre) in ZONAS.iter().enumerate() {
                if let Some(v) = a.valor(&k(nombre)) {
                    b.zonas[z] = leer_zona(v, &mut avisos, &k(nombre));
                }
            }
            normalizar(&mut b, &mut avisos);
            c.barras.push(b);
        }
        // Claves que no son de nadie: se dicen (se quedan en el fichero).
        for (clave, _) in a.todos() {
            let suya = CLAVES_GLOBALES.iter().any(|g| g.eq_ignore_ascii_case(clave))
                || c.barras.iter().any(|b| clave.len() > b.id.len() + 1 && clave[..b.id.len()].eq_ignore_ascii_case(&b.id) && clave.as_bytes()[b.id.len()] == b'-' && CLAVES_BARRA.iter().any(|x| x.eq_ignore_ascii_case(&clave[b.id.len() + 1..])));
            if !suya {
                avisos.push(format!("{}: no se conoce (o es de una barra que no está en \"barras\"); se deja", clave));
            }
        }
        (c, avisos)
    }

    /// El fichero en texto, sobre lo que había (`antes`): toca solo las líneas de las claves (las demás, comentarios
    /// incluidos, se quedan) y quita las de barras que ya no están.
    pub fn escribir(&self, antes: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let mut a = match antes.map(Ajustes::leer) {
            Some(Ok(a)) => a,
            _ => {
                let mut a = Ajustes::nuevo();
                a.comentar(CABECERA);
                a
            }
        };
        let poner = |a: &mut Ajustes, k: &str, v: &str| a.poner(k, v).map_err(|e| format!("{} no cabe en el fichero ({:?})", k, e));
        let ids: Vec<&str> = self.barras.iter().map(|b| b.id.as_str()).collect();
        // Lo de barras que ya no están, fuera.
        let viejas: Vec<String> = a
            .todos()
            .filter_map(|(k, _)| {
                let (id, campo) = k.split_once('-')?;
                (CLAVES_BARRA.contains(&campo) && !ids.iter().any(|x| x.eq_ignore_ascii_case(id))).then(|| k.to_string())
            })
            .collect();
        for k in viejas {
            a.quitar(&k);
        }
        poner(&mut a, "barras", &ids.join(", "))?;
        poner(&mut a, "radios", &self.radios.to_string())?;
        poner(&mut a, "radio_ventanas", &self.radio_ventanas.to_string())?;
        poner(&mut a, "estilo", if self.clasico { "clasico" } else { "expresivo" })?;
        poner(&mut a, "formato_fecha", &self.formato_fecha)?;
        for b in &self.barras {
            if !id_valido(&b.id) {
                return Err(format!("\"{}\" no vale como nombre de barra", b.id));
            }
            let k = |campo: &str| format!("{}-{}", b.id, campo);
            let mons = match &b.monitores {
                Monitores::Todos => String::from("todos"),
                Monitores::Estos(v) => v.join(", "),
            };
            let pares: [(&str, String); 28] = [
                ("nombre", b.nombre.clone()),
                ("activa", texto_si(b.activa).into()),
                ("visible", texto_si(b.visible).into()),
                ("posicion", b.posicion.nombre().into()),
                ("monitores", mons),
                ("ultimo_monitor", texto_si(b.ultimo_monitor).into()),
                ("espaciado", b.espaciado.to_string()),
                ("relleno", b.relleno.to_string()),
                ("hueco", b.hueco.to_string()),
                ("opacidad", b.opacidad.to_string()),
                ("opacidad_widgets", b.opacidad_widgets.to_string()),
                ("sin_fondo", texto_si(b.sin_fondo).into()),
                ("esquinas_rectas", texto_si(b.esquinas_rectas).into()),
                ("pegada", texto_si(b.pegada).into()),
                ("borde", texto_si(b.borde).into()),
                ("borde_color", b.borde_color.nombre().into()),
                ("borde_opacidad", b.borde_opacidad.to_string()),
                ("borde_grosor", b.borde_grosor.to_string()),
                ("contorno", texto_si(b.contorno).into()),
                ("contorno_color", b.contorno_color.nombre().into()),
                ("contorno_opacidad", b.contorno_opacidad.to_string()),
                ("contorno_grosor", b.contorno_grosor.to_string()),
                ("letra", b.letra.to_string()),
                ("iconos", b.iconos.to_string()),
                ("estilo", (if b.plano { "plano" } else { "pildoras" }).into()),
                ("autoocultar", texto_si(b.autoocultar).into()),
                ("retardo", b.retardo.to_string()),
                ("sombra", b.sombra.to_string()),
            ];
            for (campo, v) in pares {
                poner(&mut a, &k(campo), &v)?;
            }
            for (z, nombre) in ZONAS.iter().enumerate() {
                poner(&mut a, &k(nombre), &escribir_zona(&b.zonas[z]))?;
            }
        }
        Ok(a.texto().into_bytes())
    }
}

/// Lo que no puede ser (dos `menus`, una zona que no cabe en una línea): se arregla y se dice.
fn normalizar(b: &mut Barra, avisos: &mut Vec<String>) {
    let mut vistos: Vec<&'static str> = Vec::new();
    for z in 0..3 {
        b.zonas[z].retain(|w| match w.ficha() {
            Some(f) if f.uno => {
                if vistos.contains(&f.id) {
                    avisos.push(format!("{}: {} solo puede estar una vez; el de {} sobra", b.id, f.id, ZONAS[z]));
                    false
                } else {
                    vistos.push(f.id);
                    true
                }
            }
            _ => true,
        });
        while b.zonas[z].len() > MAX_WIDGETS || escribir_zona(&b.zonas[z]).len() > texto::MAX_VALOR {
            let w = b.zonas[z].pop().unwrap();
            avisos.push(format!("{}-{}: no cabe en una línea; {} sobra", b.id, ZONAS[z], w.id));
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Lo que cambia la ventana.

/// Ajustes > Barra: aplica lo pedido por el editor (`barra_editor`) a la configuración (la barra `k`; `monitores`, los que hay, para ir de uno a otro). Lo que pasó,
/// para la consola, o `None` si no se entiende.
pub fn aplicar_pedido(c: &mut Config, k: &mut usize, pedido: &str, monitores: &[String]) -> Option<String> {
    let p: Vec<&str> = pedido.split(' ').collect();
    let num = |i: usize| p.get(i).and_then(|x| x.parse::<usize>().ok());
    let kk = (*k).min(c.barras.len().saturating_sub(1));
    match p.first().copied()? {
        "barra_nueva" => {
            if c.barras.len() >= MAX_BARRAS {
                return Some(format!("ya hay {} barras (las de DMS como mucho)", MAX_BARRAS));
            }
            let id = (2..).map(|n| format!("barra{}", n)).find(|x| c.barra(x).is_none())?;
            let mut b = Barra::nueva(&id);
            b.nombre = format!("Barra {}", c.barras.len() + 1);
            // Como DMS al añadir: la nueva, abajo y sin widgets que no se vean.
            b.posicion = Posicion::Abajo;
            b.zonas = [Vec::new(), alloc::vec![Widget::nuevo("reloj")], Vec::new()];
            c.barras.push(b);
            *k = c.barras.len() - 1;
            Some(format!("barra nueva: {}", id))
        }
        "barra_quitar" => {
            if c.barras.len() <= 1 {
                return Some(String::from("la única barra no se quita (se puede desactivar)"));
            }
            let b = c.barras.remove(kk);
            *k = kk.min(c.barras.len() - 1);
            Some(format!("barra quitada: {}", b.id))
        }
        "global" if p.get(1) == Some(&"radios") => {
            c.radios = if c.radios >= 100 { 0 } else { c.radios + 10 - c.radios % 10 };
            Some(format!("radios: {}", c.radios))
        }
        "barra" => {
            let b = c.barras.get_mut(kk)?;
            let campo = *p.get(1)?;
            match campo {
                "posicion" => {
                    let i = Posicion::TODAS.iter().position(|x| *x == b.posicion).unwrap_or(0);
                    b.posicion = Posicion::TODAS[(i + 1) % 4];
                }
                "monitores" => {
                    // Todos, luego cada monitor que hay, y vuelta a todos.
                    b.monitores = match &b.monitores {
                        Monitores::Todos if !monitores.is_empty() => Monitores::Estos(alloc::vec![monitores[0].clone()]),
                        Monitores::Estos(v) => match monitores.iter().position(|m| v.first() == Some(m)) {
                            Some(i) if i + 1 < monitores.len() => Monitores::Estos(alloc::vec![monitores[i + 1].clone()]),
                            _ => Monitores::Todos,
                        },
                        _ => Monitores::Todos,
                    };
                }
                "activa" => b.activa = !b.activa,
                "relleno" => b.relleno = if b.relleno >= 12 { 0 } else { b.relleno + 2 },
                "opacidad" => b.opacidad = if b.opacidad >= 100 { 50 } else { (b.opacidad + 10).min(100) },
                "esquinas" => b.esquinas_rectas = !b.esquinas_rectas,
                _ => return None,
            }
            Some(format!("{}: {}", b.id, campo))
        }
        accion => {
            let b = c.barras.get_mut(kk)?;
            let (z, n) = (num(1)?, num(2));
            if z > 2 {
                return None;
            }
            match accion {
                "mover" => {
                    let (n, z2, n2) = (n?, num(3)?, num(4)?);
                    if z2 > 2 || n >= b.zonas[z].len() {
                        return None;
                    }
                    if z2 != z && b.zonas[z2].len() >= MAX_WIDGETS {
                        return Some(format!("{} ya tiene {} widgets", ZONAS[z2], MAX_WIDGETS));
                    }
                    let w = b.zonas[z].remove(n);
                    let n2 = if z2 == z && n2 > n { n2 - 1 } else { n2 }.min(b.zonas[z2].len());
                    let id = w.id.clone();
                    b.zonas[z2].insert(n2, w);
                    Some(format!("{} de {} {} a {} {}", id, ZONAS[z], n, ZONAS[z2], n2))
                }
                "alternar" => {
                    let w = b.zonas[z].get_mut(n?)?;
                    w.activo = !w.activo;
                    Some(format!("{} {}", w.id, if w.activo { "encendido" } else { "apagado" }))
                }
                "quitar" => {
                    let n = n?;
                    (n < b.zonas[z].len()).then(|| format!("{} quitado", b.zonas[z].remove(n).id))
                }
                "opcion" => {
                    let w = b.zonas[z].get_mut(n?)?;
                    let (clave, valor) = (*p.get(3)?, *p.get(4)?);
                    w.poner(clave, valor);
                    Some(format!("{}: {} = {}", w.id, clave, valor))
                }
                "elegido" => {
                    let f = ficha_de(p.get(2)?)?;
                    if b.zonas[z].len() >= MAX_WIDGETS {
                        return Some(format!("{} ya tiene {} widgets", ZONAS[z], MAX_WIDGETS));
                    }
                    if f.uno && b.zonas.iter().flatten().any(|w| w.id == f.id) {
                        return Some(format!("{} solo puede estar una vez", f.id));
                    }
                    b.zonas[z].push(Widget::nuevo(f.id));
                    Some(format!("{} añadido a {}", f.id, ZONAS[z]))
                }
                _ => None,
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El importador de DMS.

/// Lo que se usa de lo de fuera de `barConfigs` en `settings.json`.
const GLOBALES_DMS: &[&str] = &["radiusStrength", "niriLayoutRadiusOverride", "interfaceStyle", "clockDateFormat", "barElevationEnabled", "popupTransparency"];

/// Los campos de un `barConfig` que se usan.
const CAMPOS_USADOS: &[&str] = &[
    "id", "name", "enabled", "visible", "position", "screenPreferences", "showOnLastDisplay", "spacing", "innerPadding", "bottomGap", "transparency",
    "widgetTransparency", "followInterfaceStyle", "widgetFollowInterfaceStyle", "noBackground", "squareCorners", "attachToScreenEdge", "borderEnabled",
    "borderColor", "borderOpacity", "borderThickness", "widgetOutlineEnabled", "widgetOutlineColor", "widgetOutlineOpacity", "widgetOutlineThickness",
    "fontScale", "iconScale", "widgetStyle", "autoHide", "autoHideDelay", "shadowIntensity", "leftWidgets", "centerWidgets", "rightWidgets",
];

/// Lee un `settings.json` de DMS y saca las barras. `Err` si no es JSON o no trae barras; si no, la configuración y lo
/// que se dejó fuera (dicho en llano, una cosa por línea).
pub fn importar_dms(bytes: &[u8]) -> Result<(Config, Vec<String>), String> {
    let raiz = json::leer(bytes).map_err(|e| format!("no es JSON (byte {}: {})", e.en, e.porque))?;
    let Json::Objeto(claves) = &raiz else { return Err(String::from("no es un settings.json de DMS: no es un objeto")) };
    let Some(lista) = raiz.campo("barConfigs").and_then(Json::lista) else {
        return Err(String::from("no trae barConfigs (las barras de DMS)"));
    };
    let mut dice = Vec::new();
    let mut c = Config::de_serie();
    c.barras.clear();
    // Lo global que usa la barra.
    if let Some(n) = raiz.campo("radiusStrength").and_then(Json::num) {
        c.radios = n.clamp(0.0, 100.0) as u32;
    }
    if let Some(n) = raiz.campo("niriLayoutRadiusOverride").and_then(Json::num) {
        c.radio_ventanas = (n as i64).clamp(-1, 64) as i32;
    }
    c.clasico = raiz.campo("interfaceStyle").and_then(Json::texto) == Some("classic");
    if let Some(f) = raiz.campo("clockDateFormat").and_then(Json::texto) {
        c.formato_fecha = f.chars().filter(|c| !c.is_control()).take(64).collect();
    }
    // Sin elevación en las barras: sin sombra (si una barra pide la suya, `shadowIntensity`, la lleva).
    let elevacion = raiz.campo("barElevationEnabled").and_then(Json::bool).unwrap_or(true);
    let popup = raiz.campo("popupTransparency").and_then(Json::num).unwrap_or(1.0);
    let otras = claves.iter().filter(|(k, _)| k != "barConfigs" && !GLOBALES_DMS.contains(&k.as_str())).count();
    if otras > 0 {
        dice.push(format!("de lo demás del fichero ({} claves: dock, centro de control, avisos, fondos...) la barra no usa nada", otras));
    }
    let mut ignorados: Vec<String> = Vec::new();
    for (n, bc) in lista.iter().enumerate() {
        let Json::Objeto(campos) = bc else {
            dice.push(format!("la barra {} no es un objeto: fuera", n + 1));
            continue;
        };
        if c.barras.len() >= MAX_BARRAS {
            dice.push(format!("más de {} barras: la {} y las de después, fuera", MAX_BARRAS, n + 1));
            break;
        }
        if bc.campo("island").and_then(Json::bool) == Some(true) || bc.campo("dot").and_then(Json::bool) == Some(true) {
            dice.push(format!("la barra {} es una isla de DMS, que DashOS no tiene: fuera", n + 1));
            continue;
        }
        // El nombre: el de DMS ("default" pasa a "principal"), solo letras y cifras; repetido o vacío, "barraN".
        let crudo = bc.campo("id").and_then(Json::texto).unwrap_or("");
        let mut id: String = if crudo == "default" { String::from("principal") } else { crudo.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(MAX_ID).collect() };
        if id.is_empty() || c.barras.iter().any(|b| b.id.eq_ignore_ascii_case(&id)) {
            let nuevo = (1..).map(|k| format!("barra{}", k)).find(|x| !c.barras.iter().any(|b| b.id == *x)).unwrap();
            if !crudo.is_empty() {
                dice.push(format!("la barra \"{}\" pasa a llamarse {} (repetida o con un nombre que no vale)", crudo, nuevo));
            }
            id = nuevo;
        }
        let mut b = Barra::nueva(&id);
        if let Some(t) = bc.campo("name").and_then(Json::texto) {
            b.nombre = t.chars().filter(|c| !c.is_control()).take(MAX_NOMBRE).collect();
        }
        let sino = |k: &str, serie: bool| bc.campo(k).and_then(Json::bool).unwrap_or(serie);
        let entero = |k: &str, serie: i32, min: i32, max: i32| bc.campo(k).and_then(Json::num).map_or(serie, |n| redondo(n).clamp(min as i64, max as i64) as i32);
        let por_ciento = |k: &str, serie: u32| bc.campo(k).and_then(Json::num).map_or(serie, |n| redondo(n * 100.0).clamp(0, 1000) as u32);
        b.activa = sino("enabled", true);
        b.visible = sino("visible", true);
        b.posicion = match bc.campo("position").and_then(Json::num).map(|n| n as i64) {
            None | Some(0) => Posicion::Arriba,
            Some(1) => Posicion::Abajo,
            Some(2) => Posicion::Izquierda,
            Some(3) => Posicion::Derecha,
            Some(p @ 4..=7) => {
                dice.push(format!("{}: la posición centrada de DMS ({}) va al borde, a lo ancho", id, p));
                Posicion::TODAS[(p - 4) as usize]
            }
            Some(p) => {
                dice.push(format!("{}: posición {} desconocida: arriba", id, p));
                Posicion::Arriba
            }
        };
        if let Some(l) = bc.campo("screenPreferences").and_then(Json::lista) {
            let nombres: Vec<String> = l.iter().filter_map(|m| m.texto().map(String::from).or_else(|| m.campo("name").and_then(Json::texto).map(String::from))).collect();
            if nombres.is_empty() || nombres.iter().any(|m| m == "all") {
                b.monitores = Monitores::Todos;
            } else {
                let ms: Vec<String> = nombres.iter().map(|m| m.chars().filter(|c| !c.is_control() && *c != ',').take(32).collect::<String>()).filter(|m| !m.is_empty()).take(MAX_MONITORES).collect();
                dice.push(format!("{}: sus monitores ({}) son de Linux; en DashOS se llaman DASHOS-1, DASHOS-2... y hay que elegirlos en Ajustes", id, ms.join(", ")));
                b.monitores = Monitores::Estos(ms);
            }
        }
        b.ultimo_monitor = sino("showOnLastDisplay", true);
        b.espaciado = entero("spacing", 4, 0, MAX_ESPACIADO);
        b.relleno = entero("innerPadding", 2, 0, MAX_RELLENO);
        b.hueco = entero("bottomGap", 0, 0, MAX_HUECO);
        // La opacidad: con "seguir el estilo", la de los paneles (popupTransparency); los widgets, opacos.
        b.opacidad = if sino("followInterfaceStyle", true) { redondo(popup * 100.0).clamp(0, 100) as u32 } else { por_ciento("transparency", 100).min(100) };
        b.opacidad_widgets = if sino("widgetFollowInterfaceStyle", true) { 100 } else { por_ciento("widgetTransparency", 100).min(100) };
        b.sin_fondo = sino("noBackground", false);
        b.esquinas_rectas = sino("squareCorners", false);
        b.pegada = sino("attachToScreenEdge", false);
        b.borde = sino("borderEnabled", false);
        b.contorno = sino("widgetOutlineEnabled", false);
        for (k, destino) in [("borderColor", &mut b.borde_color), ("widgetOutlineColor", &mut b.contorno_color)] {
            if let Some(t) = bc.campo(k).and_then(Json::texto) {
                match Papel::de_dms(t) {
                    Some(p) => *destino = p,
                    None => dice.push(format!("{}: el color {} = {} no es un papel del tema; el de serie", id, k, t)),
                }
            }
        }
        b.borde_opacidad = por_ciento("borderOpacity", 100).min(100);
        b.borde_grosor = entero("borderThickness", 1, 1, 8);
        b.contorno_opacidad = por_ciento("widgetOutlineOpacity", 100).min(100);
        b.contorno_grosor = entero("widgetOutlineThickness", 1, 1, 8);
        b.letra = por_ciento("fontScale", 100).clamp(50, 200);
        b.iconos = por_ciento("iconScale", 100).clamp(50, 200);
        b.plano = bc.campo("widgetStyle").and_then(Json::texto) == Some("flat");
        b.autoocultar = sino("autoHide", false);
        b.retardo = entero("autoHideDelay", 250, 0, 10_000) as u32;
        b.sombra = if elevacion || bc.campo("shadowIntensity").and_then(Json::num).unwrap_or(0.0) > 0.0 { entero("shadowIntensity", 0, 0, 100) as u32 } else { 0 };
        for (z, k) in ["leftWidgets", "centerWidgets", "rightWidgets"].iter().enumerate() {
            b.zonas[z] = match bc.campo(k).and_then(Json::lista) {
                Some(l) => importar_zona(l, &id, ZONAS[z], &mut dice),
                None => Vec::new(),
            };
        }
        for (k, _) in campos {
            if !CAMPOS_USADOS.contains(&k.as_str()) && !ignorados.contains(k) {
                ignorados.push(k.clone());
            }
        }
        normalizar(&mut b, &mut dice);
        c.barras.push(b);
    }
    if c.barras.is_empty() {
        return Err(String::from("barConfigs no trae ninguna barra que valga"));
    }
    // Los menús del programa (de DashOS, DMS no los tiene): tras la ventana de la primera barra de arriba; si no, no se
    // verían nunca con los menús en la barra.
    if !c.barras.iter().any(|b| b.zonas.iter().flatten().any(|w| w.id == "menus"))
        && let Some(b) = c.barras.iter_mut().find(|b| b.posicion == Posicion::Arriba)
    {
        let sitio = b.zonas[0].iter().position(|w| w.id == "ventana").map_or(b.zonas[0].len(), |i| i + 1);
        if b.zonas[0].len() < MAX_WIDGETS {
            b.zonas[0].insert(sitio, Widget::nuevo("menus"));
            dice.push(format!("{}: se añaden los menús del programa tras la ventana (DMS no los tiene; solo se ven con los menús en la barra)", b.id));
        }
    }
    if !ignorados.is_empty() {
        dice.push(format!("de cada barra, DashOS no usa: {}", ignorados.join(", ")));
    }
    Ok((c, dice))
}

/// El color de DMS en `settings.json`, como el acento de DashOS (`ENV:Escritorio/apariencia`): con el tema "dynamic"
/// (los colores del fondo, con matugen), `scheme-monochrome` es el monocromo y los demás esquemas, la paleta del fondo.
/// Un tema de DMS con nombre (los de su catálogo) no lo tiene DashOS: `None`, y se dice. El acento y lo que se dice.
pub fn acento_dms(bytes: &[u8]) -> Option<(&'static str, String)> {
    let raiz = json::leer(bytes).ok()?;
    let tema = raiz.campo("currentThemeName").and_then(Json::texto)?;
    let esquema = raiz.campo("matugenScheme").and_then(Json::texto).unwrap_or("scheme-tonal-spot");
    match (tema, esquema) {
        ("dynamic", "scheme-monochrome") => Some(("monocromo", String::from("el color: monocromo (DMS: dinámico con el esquema scheme-monochrome de matugen)"))),
        ("dynamic", e) => Some(("fondo", format!("el color: la paleta del fondo (DMS: dinámico con {}; DashOS la saca a su manera)", e))),
        (t, _) => Some(("", format!("el tema de DMS \"{}\" no lo tiene DashOS: el color se queda como está", t.chars().filter(|c| !c.is_control()).take(40).collect::<String>()))),
    }
}

/// Redondea sin `f64::round` (que no está sin `std`); los valores enormes se quedan en el tope de `i64`.
fn redondo(x: f64) -> i64 {
    if x.is_nan() { 0 } else if x >= 0.0 { (x + 0.5) as i64 } else { (x - 0.5) as i64 }
}

fn importar_zona(l: &[Json], barra: &str, zona: &str, dice: &mut Vec<String>) -> Vec<Widget> {
    let mut v = Vec::new();
    for e in l {
        let (dms, activo, campos): (&str, bool, &[(String, Json)]) = match e {
            Json::Texto(t) => (t, true, &[]),
            Json::Objeto(c) => match e.campo("id").and_then(Json::texto) {
                Some(id) => (id, e.campo("enabled").and_then(Json::bool).unwrap_or(true), c),
                None => {
                    dice.push(format!("{} ({}): un widget sin id: fuera", barra, zona));
                    continue;
                }
            },
            _ => {
                dice.push(format!("{} ({}): un widget que no es ni nombre ni objeto: fuera", barra, zona));
                continue;
            }
        };
        let Some(f) = ficha_de_dms(dms) else {
            let visto: String = dms.chars().filter(|c| !c.is_control()).take(40).collect();
            dice.push(format!("{} ({}): {} no existe en DashOS (es un complemento de DMS o de otro escritorio): fuera", barra, zona, visto));
            continue;
        };
        if v.len() >= MAX_WIDGETS {
            dice.push(format!("{} ({}): más de {} widgets; {} y los de después, fuera", barra, zona, MAX_WIDGETS, f.id));
            break;
        }
        let mut w = Widget { id: f.id.to_string(), activo, opciones: Vec::new() };
        let mut sin: Vec<&str> = Vec::new();
        for (k, val) in campos {
            if k == "id" || k == "enabled" {
                continue;
            }
            let Some(o) = f.opciones.iter().find(|o| o.dms == k) else {
                sin.push(k);
                continue;
            };
            let valor = match (o.tipo, val) {
                (Tipo::SiNo, Json::Bool(b)) => Some(String::from(texto_si(*b))),
                (Tipo::Numero(a, z), Json::Num(n)) => Some((redondo(*n).clamp(a as i64, z as i64)).to_string()),
                (Tipo::Uno(vs), Json::Texto(t)) => match o.dms_valores.iter().position(|x| x == t) {
                    Some(i) => vs.get(i).map(|x| String::from(*x)),
                    None => {
                        dice.push(format!("{}: {} = {} no lo tiene DashOS; vale {}", f.id, k, t, o.serie));
                        None
                    }
                },
                _ => {
                    dice.push(format!("{}: {} trae un valor que no es del tipo: lo de serie", f.id, k));
                    None
                }
            };
            if let Some(valor) = valor {
                w.poner(o.clave, &valor);
            }
        }
        if !sin.is_empty() {
            dice.push(format!("{}: opciones que DashOS no tiene: {}", f.id, sin.join(", ")));
        }
        if let Some(porque) = f.falta {
            dice.push(format!("{}: se guarda, pero no se ve hasta que haya de dónde ({})", f.id, porque));
        }
        v.push(w);
    }
    v
}

// ---------------------------------------------------------------------------------------------------------------
// La fecha, con el formato de DMS.

/// Los nombres largos de los días (lunes en 0) y de los meses.
pub const DIAS_LARGOS: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
pub const MESES_LARGOS: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
pub const DIAS_CORTOS: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
pub const MESES_CORTOS: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];

/// La fecha con un formato de Qt (el de `clockDateFormat`): d, dd, ddd, dddd, M, MM, MMM, MMMM, yy, yyyy; lo demás,
/// tal cual; entre comillas simples, texto. `semana` 0 es el lunes; `mes` de 1 a 12.
pub fn formato_fecha(formato: &str, anio: i64, mes: u32, dia: u32, semana: u32) -> String {
    let formato = if formato.trim().is_empty() { "ddd d MMM" } else { formato };
    let mut s = String::new();
    let c: Vec<char> = formato.chars().collect();
    let mut i = 0;
    let mes = mes.clamp(1, 12) as usize;
    let semana = semana.min(6) as usize;
    while i < c.len() && s.len() < 128 {
        let x = c[i];
        let mut n = 1;
        while i + n < c.len() && c[i + n] == x {
            n += 1;
        }
        match (x, n) {
            ('d', 1) => s.push_str(&dia.to_string()),
            ('d', 2) => s.push_str(&format!("{:02}", dia)),
            ('d', 3) => s.push_str(DIAS_CORTOS[semana]),
            ('d', _) => s.push_str(DIAS_LARGOS[semana]),
            ('M', 1) => s.push_str(&mes.to_string()),
            ('M', 2) => s.push_str(&format!("{:02}", mes)),
            ('M', 3) => s.push_str(MESES_CORTOS[mes - 1]),
            ('M', _) => s.push_str(MESES_LARGOS[mes - 1]),
            ('y', 2) | ('y', 3) => s.push_str(&format!("{:02}", anio.rem_euclid(100))),
            ('y', _) if n >= 4 => s.push_str(&anio.to_string()),
            ('\'', _) => {
                // Texto entre comillas simples, hasta la siguiente.
                let fin = c[i + 1..].iter().position(|&y| y == '\'').map_or(c.len(), |p| i + 1 + p);
                s.extend(&c[i + 1..fin]);
                i = fin + 1;
                continue;
            }
            _ => {
                for _ in 0..n {
                    s.push(x);
                }
            }
        }
        i += n;
    }
    s
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Los ajustes de la barra de DMS del usuario (de shiva), sin nada que no sea de las barras más un par de claves
    /// globales que se usan y otras que no.
    const SHIVA: &str = include_str!("../pruebas/dms-barra-shiva.json");

    #[test]
    fn lo_de_serie_y_sus_medidas() {
        let c = Config::de_serie();
        let b = &c.barras[0];
        // Las fórmulas de DMS: con relleno 0, widgets de 26 y barra de 36; con 4 hueco, los 40 de siempre.
        assert_eq!((b.grosor_widgets(), b.grosor(), b.zona_exclusiva()), (26, 36, 40));
        let mut b2 = b.clone();
        b2.relleno = 4;
        // Shiva: innerPadding 4 → widgets de 28, barra de 40, zona de 44.
        assert_eq!((b2.grosor_widgets(), b2.grosor(), b2.zona_exclusiva()), (28, 40, 44));
        b2.relleno = 2;
        assert_eq!((b2.grosor_widgets(), b2.grosor()), (28, 38));
        // Radios: con 50, los de Material; con 38 (shiva), 12 en vez de 16; la píldora de 28, 11.
        assert_eq!(c.radio(16), 16);
        let mut c2 = c.clone();
        c2.radios = 38;
        assert_eq!((c2.radio(16), c2.radio_pildora(&b2, 28)), (12, 11));
        c2.radio_ventanas = 3;
        assert_eq!(c2.radio_barra(&b2), 3);
        // Escribir y leer da lo mismo.
        let bytes = c.escribir(None).unwrap();
        let (otra, avisos) = Config::leer(&bytes);
        assert_eq!(otra, c);
        assert!(avisos.is_empty(), "{:?}", avisos);
    }

    #[test]
    fn un_fichero_a_mano() {
        let t = "# a mano\nprincipal-posicion = abajo\nprincipal-izquierda = lanzador, -espacios(numeros cuantos=5), reloj(orden=fecha)\n";
        let (c, avisos) = Config::leer(t.as_bytes());
        assert!(avisos.is_empty(), "{:?}", avisos);
        let b = &c.barras[0];
        assert_eq!(b.posicion, Posicion::Abajo);
        assert_eq!(b.zonas[0].len(), 3);
        assert!(!b.zonas[0][1].activo);
        assert!(b.zonas[0][1].si("numeros"));
        assert_eq!(b.zonas[0][1].numero("cuantos"), 5);
        assert_eq!(b.zonas[0][2].opcion("orden"), Some("fecha"));
        // Lo demás, de serie.
        assert_eq!(b.zonas[2], Barra::nueva("x").zonas[2]);
        // Escribir sobre lo de antes: el comentario se queda, y vuelve a leerse igual.
        let nuevo = c.escribir(Some(t.as_bytes())).unwrap();
        let s = core::str::from_utf8(&nuevo).unwrap();
        assert!(s.starts_with("# a mano\nprincipal-posicion = abajo\n"), "{}", s);
        assert!(s.contains("principal-izquierda = lanzador, -espacios(numeros cuantos=5), reloj(orden=fecha)"), "{}", s);
        assert_eq!(Config::leer(&nuevo).0, c);
    }

    #[test]
    fn hostil() {
        // No es texto, enorme, roto: lo de serie, dicho.
        for malo in [&b"\xff\xfe"[..], &alloc::vec![b'a'; 70_000][..]] {
            let (c, avisos) = Config::leer(malo);
            assert_eq!(c, Config::de_serie());
            assert_eq!(avisos.len(), 1);
        }
        // Widgets desconocidos, opciones que no existen, valores malos, paréntesis sin cerrar, nombres raros.
        let t = "principal-izquierda = lanzador(color=rojo), fantasma, espacios(cuantos=99 estilo=raro), reloj(orden, ventana,  , ñandú, -\nprincipal-relleno = -5\nprincipal-posicion = diagonal\nzzz = 1\nbarras = principal, principal, mala!, otra, b3, b4, b5\n";
        let (c, avisos) = Config::leer(t.as_bytes());
        let b = &c.barras[0];
        assert_eq!(c.barras.len(), 4, "{:?}", avisos);
        assert_eq!(b.relleno, 0);
        assert_eq!(b.posicion, Posicion::Arriba);
        // Se guardan el fantasma y la opción que no existe; el reloj sin cerrar y lo que no es nombre, no.
        let ids: Vec<&str> = b.zonas[0].iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["lanzador", "fantasma", "espacios"]);
        assert!(!b.zonas[0][1].se_ve());
        assert_eq!(b.zonas[0][2].numero("cuantos"), 9);
        for frase in ["no tiene la opción \"color\"", "\"fantasma\" no existe", "no vale para cuantos", "paréntesis", "zzz", "dos veces", "mala!", "más de 4", "diagonal", "fuera de 0 a 24"] {
            assert!(avisos.iter().any(|a| a.contains(frase)), "falta «{}» en {:?}", frase, avisos);
        }
        // Mil widgets: los primeros que caben, y se dice.
        let mil: Vec<&str> = (0..200).map(|_| "cpu").collect();
        let t = format!("principal-derecha = {}\n", mil.join(","));
        let (c, avisos) = Config::leer(t.as_bytes());
        assert_eq!(c.barras[0].zonas[2].len(), MAX_WIDGETS);
        assert!(avisos.iter().any(|a| a.contains("más de 24")));
        // Y mil no caben en una línea del fichero: la línea no vale (se dice) y la zona queda la de serie.
        let mil: Vec<&str> = (0..1000).map(|_| "cpu").collect();
        let t = format!("principal-derecha = {}\n", mil.join(","));
        let (c, avisos) = Config::leer(t.as_bytes());
        assert_eq!(c.barras[0].zonas[2], Barra::nueva("x").zonas[2]);
        assert!(avisos.iter().any(|a| a.contains("línea 1")), "{:?}", avisos);
        // Dos menús en una barra: uno.
        let (c, avisos) = Config::leer(b"principal-izquierda = menus, menus\nprincipal-derecha = menus\n");
        assert_eq!(c.barras[0].zonas.iter().flatten().filter(|w| w.id == "menus").count(), 1);
        assert_eq!(avisos.len(), 2, "{:?}", avisos);
        // Al escribir, lo de una barra que se quitó se va; lo que no es de nadie se queda.
        let (mut c, _) = Config::leer(b"barras = a, b\nb-posicion = abajo\nnota = algo\n");
        c.barras.retain(|b| b.id == "a");
        let s = String::from_utf8(c.escribir(Some(b"barras = a, b\nb-posicion = abajo\nnota = algo\n")).unwrap()).unwrap();
        assert!(!s.contains("b-posicion") && s.contains("nota = algo") && s.contains("barras = a\n"), "{}", s);
        // Bytes al azar como fichero: nunca un pánico.
        let mut x = 99u32;
        for _ in 0..3000 {
            let mut v = Vec::new();
            for _ in 0..(x % 200) {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                v.push(b"principal-izquierda=(), \"-cpu\nbarras#a"[(x as usize) % 38]);
            }
            let (c, _) = Config::leer(&v);
            let _ = c.escribir(Some(&v));
        }
    }

    #[test]
    fn pedidos_del_editor() {
        let mut c = Config::de_serie();
        let mut k = 0;
        let mon = [String::from("DASHOS-1"), String::from("DASHOS-2")];
        // Mover el reloj (centro 1) al principio de la derecha, apagarlo, una opción, quitar y añadir.
        assert!(aplicar_pedido(&mut c, &mut k, "mover 1 1 2 0", &mon).is_some());
        assert_eq!(c.barras[0].zonas[2][0].id, "reloj");
        assert_eq!(c.barras[0].zonas[1].len(), 2);
        // Dentro de la misma zona, hacia abajo: queda donde se soltó.
        aplicar_pedido(&mut c, &mut k, "mover 0 0 0 3", &mon);
        assert_eq!(c.barras[0].zonas[0].iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["espacios", "ventana", "lanzador", "menus"]);
        aplicar_pedido(&mut c, &mut k, "alternar 2 0", &mon);
        assert!(!c.barras[0].zonas[2][0].activo);
        aplicar_pedido(&mut c, &mut k, "opcion 2 0 compacto sí", &mon);
        assert!(c.barras[0].zonas[2][0].si("compacto"));
        aplicar_pedido(&mut c, &mut k, "quitar 2 0", &mon);
        aplicar_pedido(&mut c, &mut k, "elegido 1 disco", &mon);
        assert_eq!(c.barras[0].zonas[1].last().unwrap().id, "disco");
        assert!(aplicar_pedido(&mut c, &mut k, "elegido 1 menus", &mon).unwrap().contains("una vez"));
        // Monitores: todos, DASHOS-1, DASHOS-2 y vuelta.
        for esperado in [Monitores::Estos(alloc::vec![mon[0].clone()]), Monitores::Estos(alloc::vec![mon[1].clone()]), Monitores::Todos] {
            aplicar_pedido(&mut c, &mut k, "barra monitores", &mon);
            assert_eq!(c.barras[0].monitores, esperado);
        }
        // Barras nuevas hasta el tope; quitar la elegida; la última no se quita.
        for _ in 0..5 {
            aplicar_pedido(&mut c, &mut k, "barra_nueva", &mon);
        }
        assert_eq!(c.barras.len(), MAX_BARRAS);
        assert_eq!(k, MAX_BARRAS - 1);
        for _ in 0..5 {
            aplicar_pedido(&mut c, &mut k, "barra_quitar", &mon);
        }
        assert_eq!(c.barras.len(), 1);
        // Hostil: pedidos que no se entienden, índices fuera, zonas que no existen: nada, sin pánico.
        for malo in ["", "mover", "mover 9 9 9 9", "mover 0 99 1 0", "alternar 3 0", "opcion 0 0", "elegido 0 fantasma", "barra nada", "quitar 0 999", "x y z"] {
            let antes = c.clone();
            let _ = aplicar_pedido(&mut c, &mut k, malo, &mon);
            assert_eq!(c, antes, "{}", malo);
        }
        // Lo cambiado se escribe y se lee igual.
        assert_eq!(Config::leer(&c.escribir(None).unwrap()).0, c);
    }

    #[test]
    fn monitores() {
        let mut b = Barra::nueva("x");
        assert!(b.en_monitor("DASHOS-2", &["DASHOS-1", "DASHOS-2"]));
        b.monitores = Monitores::Estos(alloc::vec![String::from("DASHOS-2")]);
        assert!(!b.en_monitor("DASHOS-1", &["DASHOS-1", "DASHOS-2"]));
        assert!(b.en_monitor("dashos-2", &["DASHOS-1", "DASHOS-2"]));
        // Un monitor que no existe: con un solo monitor y `ultimo_monitor`, en ese; con dos, en ninguno.
        b.monitores = Monitores::Estos(alloc::vec![String::from("HDMI-A-3")]);
        assert!(b.en_monitor("DASHOS-1", &["DASHOS-1"]));
        assert!(!b.en_monitor("DASHOS-1", &["DASHOS-1", "DASHOS-2"]));
        assert_eq!(b.monitores_que_faltan(&["DASHOS-1"]), [String::from("HDMI-A-3")]);
        b.ultimo_monitor = false;
        assert!(!b.en_monitor("DASHOS-1", &["DASHOS-1"]));
        b.activa = false;
        b.monitores = Monitores::Todos;
        assert!(!b.en_monitor("DASHOS-1", &["DASHOS-1"]));
    }

    #[test]
    fn importar_shiva() {
        let (c, dice) = importar_dms(SHIVA.as_bytes()).unwrap();
        assert_eq!((c.radios, c.radio_ventanas, c.clasico, c.formato_fecha.as_str()), (38, 3, false, "dddd, MMMM d"));
        // Su color: dinámico con scheme-monochrome, el monocromo.
        assert_eq!(acento_dms(SHIVA.as_bytes()).map(|x| x.0), Some("monocromo"));
        assert_eq!(acento_dms(b"{\"currentThemeName\": \"dynamic\"}").map(|x| x.0), Some("fondo"));
        assert_eq!(acento_dms(b"{\"currentThemeName\": \"catppuccin\"}").map(|x| x.0), Some(""));
        assert_eq!(acento_dms(b"{}"), None);
        assert_eq!(c.barras.len(), 1);
        let b = &c.barras[0];
        assert_eq!((b.id.as_str(), b.nombre.as_str(), b.posicion, b.espaciado, b.relleno, b.opacidad, b.sombra), ("principal", "Main Bar", Posicion::Arriba, 4, 4, 100, 0));
        assert_eq!(b.monitores, Monitores::Todos);
        let zona = |z: usize| b.zonas[z].iter().map(|w| w.id.as_str()).collect::<Vec<_>>();
        assert_eq!(zona(0), ["lanzador", "espacios", "ventana", "menus"]);
        assert_eq!(zona(1), ["musica", "reloj", "tiempo", "avisos"]);
        assert_eq!(zona(2), ["bandeja", "portapapeles", "cpu", "memoria", "temperatura", "temperatura_gpu", "bateria", "estado", "actualizaciones", "apagado"]);
        let espacios = &b.zonas[0][1];
        assert!(espacios.si("numeros") && espacios.si("ocupados") && espacios.si("borde") && espacios.si("relleno"));
        assert_eq!(b.zonas[0][0].opcion("logo"), Some("sistema"));
        // Lo que se dejó fuera, dicho.
        for frase in ["mpvpaperWallpaper no existe", "dockerManager no existe", "kubernetes no existe", "qcalCalendar no existe", "amdGpuMonitor no existe",
            "showWorkspaceName", "showMicPercent", "menús del programa", "pciId", "la barra no usa nada", "scrollEnabled"] {
            assert!(dice.iter().any(|d| d.contains(frase)), "falta «{}» en {:#?}", frase, dice);
        }
        // Y el perfil que va en el disco es justo esto.
        let perfil = c.escribir(None).unwrap();
        if std::env::var_os("REGENERAR").is_some() {
            std::fs::write("pruebas/perfil-barra-shiva.txt", &perfil).unwrap();
        }
        let guardado = include_bytes!("../pruebas/perfil-barra-shiva.txt");
        assert_eq!(String::from_utf8_lossy(&perfil), String::from_utf8_lossy(guardado), "regenerar usuario/pruebas/perfil-barra-shiva.txt");
        assert_eq!(Config::leer(&perfil).0, c);
    }

    #[test]
    fn importar_hostil() {
        assert!(importar_dms(b"").is_err());
        assert!(importar_dms(b"[]").is_err());
        assert!(importar_dms(b"{\"barConfigs\": 3}").is_err());
        assert!(importar_dms(b"{\"barConfigs\": []}").is_err());
        assert!(importar_dms(b"{\"barConfigs\": [1, \"x\"]}").is_err());
        // Ids repetidos y malos, cinco barras, una isla, valores raros, mil widgets, widgets sin id.
        let mut j = String::from("{\"barConfigs\": [");
        for k in 0..6 {
            if k > 0 {
                j.push(',');
            }
            let id = if k < 2 { "default" } else if k == 2 { "con espacios y ñ" } else { "" };
            j.push_str(&format!("{{\"id\": \"{}\", \"position\": {}, \"spacing\": -50, \"innerPadding\": 1e9, \"island\": {}, \"leftWidgets\": [", id, k * 3, k == 5));
            let mil: Vec<&str> = (0..1000).map(|_| "\"cpuUsage\"").collect();
            j.push_str(&mil.join(","));
            j.push_str(", {\"enabled\": false}, 7, {\"id\": \"clock\", \"clockCompactMode\": \"sí\"}]}");
        }
        j.push_str("], \"radiusStrength\": 1e300}");
        let (c, dice) = importar_dms(j.as_bytes()).unwrap();
        assert_eq!(c.barras.len(), MAX_BARRAS);
        assert_eq!(c.radios, 100);
        let ids: Vec<&str> = c.barras.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["principal", "barra1", "conespaciosy", "barra2"]);
        assert_eq!(c.barras[0].espaciado, 0);
        assert_eq!(c.barras[0].relleno, MAX_RELLENO);
        assert_eq!(c.barras[0].zonas[0].len(), MAX_WIDGETS);
        assert_eq!(c.barras[1].posicion, Posicion::Derecha);
        for frase in ["pasa a llamarse barra1", "más de 4 barras", "más de 24 widgets", "posición 9 desconocida"] {
            assert!(dice.iter().any(|d| d.contains(frase)), "falta «{}» en {:#?}", frase, dice);
        }
        // Lo importado se escribe y se lee igual.
        let bytes = c.escribir(None).unwrap();
        assert_eq!(Config::leer(&bytes).0, c);
    }

    #[test]
    fn fechas() {
        // 16 de noviembre de 2026, lunes.
        assert_eq!(formato_fecha("dddd, MMMM d", 2026, 11, 16, 0), "lunes, noviembre 16");
        assert_eq!(formato_fecha("", 2026, 11, 16, 0), "lun 16 nov");
        assert_eq!(formato_fecha("dd/MM/yy 'de' yyyy", 2026, 1, 2, 4), "02/01/26 de 2026");
        // Raro: sin pánico, y con tope.
        let _ = formato_fecha("'", 2026, 13, 40, 9);
        assert!(formato_fecha(&"dddd".repeat(1000), 2026, 1, 1, 0).len() < 200);
    }
}
