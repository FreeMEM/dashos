//! Las órdenes que el Shell conoce de serie y la plantilla de cada una: una sola tabla, para repartir los argumentos
//! (cada orden usa la suya) y para completar con Tab y resaltar (el Shell las mira aquí).

use alloc::string::String;
use alloc::vec::Vec;

// Las de base (`SYS:C`).
/// S6b, S36: abrir encerrado un programa instalado, como con doble clic; y la tanda de acciones de administrador.
pub const ABRIR: &str = "PROGRAMA/A,ESPERAR/S";
pub const ADMINISTRAR: &str = "ACCION/M,DESDE/K";
/// Ajustes en texto (`docs/bitacoras/ajustes-texto.md`): leer y poner un ajuste suelto.
pub const AJUSTE: &str = "NOMBRE,VALOR,GUARDAR/S,LISTA/S";
pub const ASK: &str = "PROMPT/A";
pub const ASSIGN: &str = "NAME,TARGET/M,LIST/S,ADD/S,PREPEND/S,REMOVE/S";
pub const CD: &str = "DIR";
pub const COPY: &str = "FROM/M,TO/A,ALL/S,QUIET/S";
pub const DATE: &str = "";
pub const DELETE: &str = "FILE/M/A,ALL/S,QUIET/S,FORCE/S";
pub const DIR: &str = "DIR,ALL/S,DIRS/S,FILES/S,COLOR/S,NOCOLOR/S";
pub const ECHO: &str = "/M,NOLINE/S";
pub const FILENOTE: &str = "FILE/A,COMMENT";
pub const INFO: &str = "";
pub const LIST: &str = "DIR/M,FILES/S,DIRS/S,QUICK/S,NOHEAD/S,COLOR/S,NOCOLOR/S";
pub const MAKEDIR: &str = "NAME/M/A";
/// S6b, S36: lo concedido a tus programas; dar y quitar, con la confirmación del sistema.
pub const PERMISO: &str = "DAR/S,QUITAR/S,PROGRAMA,PERMISO";
pub const PROTECT: &str = "FILE/A,FLAGS,ADD/S,SUB/S";
/// S6, S36: el registro común: leer (filtrar y seguir en vivo) y apuntar.
pub const REGISTRO: &str = "APUNTA/S,TEXTO/M,MOTIVO/K,VECES/K/N,PROGRAMA/K,CLASE/K,DESDE/K,HASTA/K,ULTIMAS/K/N,DE/K,TODO/S,SEGUIR/S";
pub const RENAME: &str = "FROM/A,TO=AS/A";
pub const TYPE: &str = "FROM/M,NUMBER/S,HEX/S";
pub const VERSION: &str = "";
pub const WAIT: &str = "TIME/N,SEC=SECS/S,MIN=MINS/S";

/// Las de base, por su nombre.
pub const BASE: &[(&str, &str)] = &[
    ("Abrir", ABRIR),
    ("Administrar", ADMINISTRAR),
    ("Ajuste", AJUSTE),
    ("Ask", ASK),
    ("Assign", ASSIGN),
    ("Cd", CD),
    ("Copy", COPY),
    ("Date", DATE),
    ("Delete", DELETE),
    ("Dir", DIR),
    ("Echo", ECHO),
    ("Filenote", FILENOTE),
    ("Info", INFO),
    ("List", LIST),
    ("MakeDir", MAKEDIR),
    ("Permiso", PERMISO),
    ("Protect", PROTECT),
    ("Registro", REGISTRO),
    ("Rename", RENAME),
    ("Type", TYPE),
    ("Version", VERSION),
    ("Wait", WAIT),
];

/// Las internas del Shell (tocan su estado).
pub const INTERNAS: &[(&str, &str)] = &[
    ("Alias", "NAME,STRING/F"),
    ("Break", "PROCESS/N"),
    ("Cls", ""),
    ("Completa", "ORDEN,PLANTILLA/F"),
    ("Completar", "LINEA/F"),
    ("Get", "NAME/A"),
    ("GetEnv", "NAME/A"),
    ("Historial", "BORRAR/S"),
    ("Medidas", "CALLA/S,HABLA/S"),
    ("NewShell", "FROM/K,REGISTRO/S,SINARRANQUE/S,ORDEN/K/F"),
    ("Panel", "HORIZONTAL/S,VERTICAL/S,PESTANA/S,CERRAR/S,SIGUIENTE/S"),
    ("Path", "PATH/M,ADD/S,SHOW/S,RESET/S"),
    ("Prompt", "PROMPT/F"),
    ("Resaltar", "LINEA/F"),
    ("Run", "COMMAND/F"),
    ("Set", "NAME,STRING/F"),
    ("SetEnv", "NAME,STRING/F"),
    ("Status", ""),
    ("Teclea", "TEXTO/F"),
    ("Unalias", "NAME/A"),
    ("Unset", "NAME/A"),
    ("UnsetEnv", "NAME/A"),
    ("Which", "FILE/A"),
    ("Why", ""),
];

/// Las de control del intérprete.
pub const CONTROL: &[(&str, &str)] = &[
    ("Else", ""),
    ("EndCLI", ""),
    ("EndIf", ""),
    ("EndShell", ""),
    ("EndSkip", ""),
    ("Execute", "FILE/A,ARGS/F"),
    ("FailAt", "RCLIM/N"),
    ("If", "NOT/S,WARN/S,ERROR/S,FAIL/S,,EQ/K,GT/K,GE/K,VAL/S,EXISTS/K"),
    ("Lab", ""),
    ("Quit", "RC/N"),
    ("Skip", "LABEL,BACK/S"),
];

/// La plantilla de la orden `nombre`, si es de las de serie.
pub fn plantilla(nombre: &str) -> Option<&'static str> {
    BASE.iter().chain(INTERNAS).chain(CONTROL).find(|(n, _)| n.eq_ignore_ascii_case(nombre)).map(|x| x.1)
}

/// Si `nombre` es interna del Shell o de control (no hace falta buscarla en el disco).
pub fn es_interna(nombre: &str) -> bool {
    INTERNAS.iter().chain(CONTROL).any(|(n, _)| n.eq_ignore_ascii_case(nombre))
}

/// Si `nombre` es una orden interna (no de control).
pub fn interna(nombre: &str) -> Option<&'static str> {
    INTERNAS.iter().find(|(n, _)| n.eq_ignore_ascii_case(nombre)).map(|x| x.1)
}

/// Una palabra de una plantilla: su nombre y si lleva un valor detrás (`/K`, o un campo con nombre) o es un interruptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palabra {
    pub nombre: String,
    pub interruptor: bool,
    /// Solo vale con su palabra delante (`/K`).
    pub clave: bool,
    /// Si conviene ofrecerla al completar: los interruptores, las `/K` y las obligatorias que no son el primer campo
    /// (`TO` de `Copy`). El nombre del primer campo (`DIR` de `Cd`) casi nunca se escribe.
    pub se_ofrece: bool,
}

/// Las palabras de una plantilla (todos los nombres de cada campo con nombre).
pub fn palabras(plantilla: &str) -> Vec<Palabra> {
    let mut v = Vec::new();
    for (k, campo) in plantilla.split(',').enumerate() {
        let mut partes = campo.split('/');
        let nombres = partes.next().unwrap_or("");
        let marcas: Vec<&str> = partes.collect();
        let interruptor = marcas.iter().any(|m| m.eq_ignore_ascii_case("S") || m.eq_ignore_ascii_case("T"));
        let clave = marcas.iter().any(|m| m.eq_ignore_ascii_case("K"));
        let obligatorio = marcas.iter().any(|m| m.eq_ignore_ascii_case("A"));
        let se_ofrece = interruptor || clave || (obligatorio && k > 0);
        for n in nombres.split('=').filter(|n| !n.is_empty()) {
            v.push(Palabra { nombre: String::from(n), interruptor, clave, se_ofrece });
        }
    }
    v
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn todas_las_plantillas_valen() {
        for (n, p) in BASE.iter().chain(INTERNAS).chain(CONTROL) {
            assert!(!matches!(crate::plantilla::leer(p, ""), Err(crate::plantilla::Error::Plantilla)), "{n}: {p}");
        }
        assert_eq!(plantilla("copy"), Some(COPY));
        assert_eq!(plantilla("nada"), None);
        assert!(es_interna("SET") && es_interna("if") && !es_interna("Dir"));
    }

    #[test]
    fn palabras_de_una_plantilla() {
        let p = palabras(COPY);
        assert_eq!(p.iter().map(|x| x.nombre.as_str()).collect::<Vec<_>>(), ["FROM", "TO", "ALL", "QUIET"]);
        assert!(p[2].interruptor && !p[0].interruptor && !p[1].clave);
        assert!(!p[0].se_ofrece && p[1].se_ofrece && p[2].se_ofrece);
        let p = palabras("FROM/A,TO=AS/K,,X/S");
        assert_eq!(p.iter().map(|x| x.nombre.as_str()).collect::<Vec<_>>(), ["FROM", "TO", "AS", "X"]);
        assert!(p[1].clave && p[2].clave);
        assert!(palabras("").is_empty());
    }
}
