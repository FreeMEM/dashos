//! Los programas de Linux como los ve un escritorio de freedesktop: sus ficheros `.desktop` (Desktop Entry
//! Specification 1.5) y dónde está su icono en un tema (Icon Theme Specification 0.13). Sin imágenes: lo usa el puente
//! dentro del invitado (con std, sin el kit) y lo prueba el PC.
//!
//! Lo leído viene de dentro de la caja: no se cree. Ficheros y líneas largas se cortan, los campos se limitan, y lo que
//! no es UTF-8 se salta.

use alloc::string::String;
use alloc::vec::Vec;

/// Lo más que se lee de un `.desktop` (los de verdad no llegan a 64 KB ni con todas las traducciones).
pub const MAX_DESKTOP: usize = 256 * 1024;
/// Lo más que mide un campo que se guarda (nombre, orden, icono).
pub const MAX_CAMPO: usize = 1024;

/// Un programa de un `.desktop`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Programa {
    /// El nombre, en el idioma pedido si está traducido.
    pub nombre: String,
    /// La orden (`Exec`), tal cual.
    pub orden: String,
    /// El icono (`Icon`): un nombre del tema o un camino entero.
    pub icono: String,
    /// Si se abre en una terminal.
    pub terminal: bool,
}

/// Quita los escapes de un valor de texto (`\s`, `\n`, `\t`, `\r`, `\\`).
fn sin_escapes(v: &str) -> String {
    let mut s = String::with_capacity(v.len());
    let mut c = v.chars();
    while let Some(x) = c.next() {
        if x != '\\' {
            s.push(x);
            continue;
        }
        match c.next() {
            Some('s') => s.push(' '),
            Some('n') => s.push('\n'),
            Some('t') => s.push('\t'),
            Some('r') => s.push('\r'),
            Some(o) => s.push(o),
            None => {}
        }
    }
    s
}

/// Cuánto casa una clave con idioma (`Name[es_ES]`) con el idioma `lang` (como `LANG`: `es_ES.UTF-8@euro`): 4 si es
/// idioma, país y variante; 3 idioma y país; 2 idioma y variante; 1 solo el idioma; 0 no casa.
fn casa_idioma(clave: &str, lang: &str) -> u8 {
    let lang = lang.split('.').next().unwrap_or("");
    let (lang, var) = match lang.split_once('@') {
        Some((l, v)) => (l, Some(v)),
        None => (lang, None),
    };
    let (idioma, pais) = match lang.split_once('_') {
        Some((i, p)) => (i, Some(p)),
        None => (lang, None),
    };
    if idioma.is_empty() {
        return 0;
    }
    let mut mejor = 0;
    for (n, c) in [
        (4, pais.zip(var).map(|(p, v)| alloc::format!("{idioma}_{p}@{v}"))),
        (3, pais.map(|p| alloc::format!("{idioma}_{p}"))),
        (2, var.map(|v| alloc::format!("{idioma}@{v}"))),
        (1, Some(String::from(idioma))),
    ] {
        if c.as_deref() == Some(clave) && n > mejor {
            mejor = n;
        }
    }
    mejor
}

/// Lee un `.desktop`. `None` si no es un programa que se enseñe (otro tipo, `NoDisplay`, `Hidden`, sin `Exec` o sin
/// nombre) o si no se puede leer.
pub fn leer_desktop(bytes: &[u8], lang: &str) -> Option<Programa> {
    // Cortado a medias de una letra (por el tope): hasta la última entera. Lo que no es UTF-8 en medio, fuera.
    let b = &bytes[..bytes.len().min(MAX_DESKTOP)];
    let texto = match core::str::from_utf8(b) {
        Ok(t) => t,
        Err(e) if e.error_len().is_none() => core::str::from_utf8(&b[..e.valid_up_to()]).ok()?,
        Err(_) => return None,
    };
    let mut en_grupo = false;
    let mut p = Programa::default();
    let (mut tipo_app, mut oculto) = (false, false);
    let mut nivel_nombre = 0u8;
    let mut hay_nombre = false;
    for linea in texto.lines() {
        let linea = linea.trim();
        if linea.is_empty() || linea.starts_with('#') {
            continue;
        }
        if linea.starts_with('[') {
            // Solo cuenta el primer grupo, el de `Desktop Entry`; las acciones (`[Desktop Action ...]`), no.
            if en_grupo {
                break;
            }
            en_grupo = linea == "[Desktop Entry]";
            continue;
        }
        if !en_grupo {
            continue;
        }
        let Some((clave, valor)) = linea.split_once('=') else { continue };
        let (clave, valor) = (clave.trim(), valor.trim());
        if valor.len() > MAX_CAMPO {
            continue;
        }
        let (base, idioma) = match clave.split_once('[') {
            Some((b, resto)) => (b, resto.strip_suffix(']')),
            None => (clave, None),
        };
        match (base, idioma) {
            ("Type", None) => tipo_app = valor == "Application",
            ("NoDisplay" | "Hidden", None) => oculto |= valor == "true",
            ("Exec", None) => p.orden = String::from(valor),
            ("Icon", None) => p.icono = sin_escapes(valor),
            ("Terminal", None) => p.terminal = valor == "true",
            ("Name", None) if !hay_nombre || nivel_nombre == 0 => {
                p.nombre = sin_escapes(valor);
                hay_nombre = true;
            }
            ("Name", Some(i)) => {
                let n = casa_idioma(i, lang);
                if n > nivel_nombre {
                    nivel_nombre = n;
                    p.nombre = sin_escapes(valor);
                    hay_nombre = true;
                }
            }
            _ => {}
        }
    }
    let nombre_vale = !p.nombre.trim().is_empty() && !p.nombre.chars().any(|c| c.is_control());
    (tipo_app && !oculto && !p.orden.is_empty() && nombre_vale).then_some(p)
}

/// La orden de un `Exec` partida en palabras, sin los códigos de campo (`%f`, `%U`...: se lanza sin ficheros) y con
/// `%%` como `%`. Las comillas dobles agrupan, y dentro de ellas `\"`, `` \` ``, `\$` y `\\` son la letra. `None` si
/// está mal (comillas sin cerrar).
pub fn palabras(exec: &str) -> Option<Vec<String>> {
    let mut v = Vec::new();
    let mut cur = String::new();
    let (mut hay, mut comillas) = (false, false);
    let mut c = exec.chars().peekable();
    while let Some(x) = c.next() {
        match x {
            '"' => {
                comillas = !comillas;
                hay = true;
            }
            '\\' if comillas => cur.push(c.next()?),
            ' ' | '\t' if !comillas => {
                if hay {
                    v.push(core::mem::take(&mut cur));
                    hay = false;
                }
            }
            '%' => match c.next()? {
                '%' => {
                    cur.push('%');
                    hay = true;
                }
                // Los códigos de campo se quitan (y una palabra que solo era eso, desaparece).
                _ => {}
            },
            o => {
                cur.push(o);
                hay = true;
            }
        }
    }
    if comillas {
        return None;
    }
    if hay {
        v.push(cur);
    }
    (!v.is_empty()).then_some(v)
}

/// El formato de un icono encontrado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Formato {
    Png,
    /// Hace falta un pintor de SVG (pendiente: lo elige la rama `iconos`).
    Svg,
}

/// Los tamaños donde buscar, del mejor al peor para un icono de `tam`: el justo, luego los más grandes (reducir se ve
/// bien), luego los más pequeños.
fn tamanos(tam: u32) -> Vec<u32> {
    const HAY: [u32; 13] = [16, 22, 24, 32, 36, 48, 64, 72, 96, 128, 192, 256, 512];
    let mut v: Vec<u32> = HAY.iter().copied().filter(|&t| t >= tam).collect();
    v.extend(HAY.iter().rev().copied().filter(|&t| t < tam));
    v
}

/// Dónde está el icono `icono` (el `Icon` de un `.desktop`) para pintarlo a `tam` píxeles: en los `temas` (en orden;
/// `hicolor` se mira siempre al final, como manda la norma) bajo cada carpeta de `bases` (`~/.local/share/icons`,
/// `/usr/share/icons`...), y si no, en `/usr/share/pixmaps`. `existe` dice si un fichero está. Primero un PNG del
/// tamaño más cercano; si solo hay SVG, el SVG.
///
/// Simplificación (apuntada): no lee el `index.theme` de cada tema ni sigue sus `Inherits`; mira las carpetas de
/// siempre (`NxN/apps`, `scalable/apps`).
pub fn buscar_icono(icono: &str, tam: u32, temas: &[&str], bases: &[&str], existe: &dyn Fn(&str) -> bool) -> Option<(String, Formato)> {
    if icono.is_empty() || icono.len() > MAX_CAMPO || icono.chars().any(|c| c.is_control()) {
        return None;
    }
    if icono.starts_with('/') {
        // Un camino entero (lo permite la norma).
        let f = if icono.ends_with(".png") {
            Formato::Png
        } else if icono.ends_with(".svg") {
            Formato::Svg
        } else {
            return None;
        };
        return existe(icono).then(|| (String::from(icono), f));
    }
    // Un nombre de tema no lleva barras ni extensión (algunos `.desktop` la ponen igual).
    if icono.contains('/') || icono == "." || icono == ".." {
        return None;
    }
    let nombre = icono.strip_suffix(".png").or_else(|| icono.strip_suffix(".svg")).or_else(|| icono.strip_suffix(".xpm")).unwrap_or(icono);
    let mut todos: Vec<&str> = temas.iter().copied().filter(|&t| t != "hicolor").collect();
    todos.push("hicolor");
    let mut svg = None;
    for tema in &todos {
        for base in bases {
            for t in tamanos(tam) {
                let c = alloc::format!("{base}/{tema}/{t}x{t}/apps/{nombre}.png");
                if existe(&c) {
                    return Some((c, Formato::Png));
                }
            }
            if svg.is_none() {
                let c = alloc::format!("{base}/{tema}/scalable/apps/{nombre}.svg");
                if existe(&c) {
                    svg = Some(c);
                }
            }
        }
    }
    let c = alloc::format!("/usr/share/pixmaps/{nombre}.png");
    if existe(&c) {
        return Some((c, Formato::Png));
    }
    if let Some(c) = svg {
        return Some((c, Formato::Svg));
    }
    let c = alloc::format!("/usr/share/pixmaps/{nombre}.svg");
    existe(&c).then_some((c, Formato::Svg))
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    const FOOT: &str = "[Desktop Entry]\nType=Application\nExec=foot\nIcon=foot\nTerminal=false\nCategories=System;TerminalEmulator;\nKeywords=shell;prompt;command;commandline;\n\nName=Foot\nGenericName=Terminal\nComment=A wayland native terminal emulator\nName[es]=Terminal Foot\n\n[Desktop Action Server]\nName=Otro\nExec=foot --server\n";

    #[test]
    fn un_desktop_de_verdad() {
        let p = leer_desktop(FOOT.as_bytes(), "es_ES.UTF-8").unwrap();
        assert_eq!(p, Programa { nombre: "Terminal Foot".to_string(), orden: "foot".to_string(), icono: "foot".to_string(), terminal: false });
        assert_eq!(leer_desktop(FOOT.as_bytes(), "C").unwrap().nombre, "Foot");
        // El nombre más concreto gana, esté donde esté.
        let t = "[Desktop Entry]\nName[es_ES]=Uno\nName=Cero\nName[es]=Dos\nType=Application\nExec=x\n";
        assert_eq!(leer_desktop(t.as_bytes(), "es_ES").unwrap().nombre, "Uno");
        assert_eq!(leer_desktop(t.as_bytes(), "es_MX").unwrap().nombre, "Dos");
        assert_eq!(leer_desktop(t.as_bytes(), "de_DE").unwrap().nombre, "Cero");
    }

    #[test]
    fn desktops_que_no_se_ensenan() {
        for malo in [
            "[Desktop Entry]\nType=Link\nName=a\nExec=b\n",
            "[Desktop Entry]\nType=Application\nName=a\nExec=b\nNoDisplay=true\n",
            "[Desktop Entry]\nType=Application\nName=a\nExec=b\nHidden=true\n",
            "[Desktop Entry]\nType=Application\nName=a\n",
            "[Desktop Entry]\nType=Application\nExec=b\n",
            "[Otra Cosa]\nType=Application\nName=a\nExec=b\n",
            "Type=Application\nName=a\nExec=b\n",
            "[Desktop Entry]\nType=Application\nName=a\u{7}\nExec=b\n",
            "",
        ] {
            assert_eq!(leer_desktop(malo.as_bytes(), "es"), None, "{malo:?}");
        }
        // Basura, nada de UTF-8, un campo enorme, y un fichero gigante: sin pánico.
        assert_eq!(leer_desktop(&[0xff, 0xfe, 0, 1, 2], "es"), None);
        let largo = alloc::format!("[Desktop Entry]\nType=Application\nExec=b\nName={}\n", "x".repeat(10_000));
        assert_eq!(leer_desktop(largo.as_bytes(), "es"), None);
        let mut enorme = b"[Desktop Entry]\nType=Application\nExec=b\nName=a\n".to_vec();
        enorme.resize(MAX_DESKTOP + 10, b'#');
        assert!(leer_desktop(&enorme, "es").is_some());
        let mut roto = b"[Desktop Entry]\nType=Application\nExec=b\nName=\xc3".to_vec();
        roto.resize(MAX_DESKTOP + 1, b'a');
        let _ = leer_desktop(&roto, "es");
    }

    #[test]
    fn ordenes() {
        assert_eq!(palabras("firefox %u"), Some(vec!["firefox".to_string()]));
        assert_eq!(palabras("gtk4-widget-factory"), Some(vec!["gtk4-widget-factory".to_string()]));
        assert_eq!(palabras("sh -c \"echo \\\"hola\\\" 100%%\" %F"), Some(vec!["sh".to_string(), "-c".to_string(), "echo \"hola\" 100%".to_string()]));
        assert_eq!(palabras("a \"sin cerrar"), None);
        assert_eq!(palabras("%U"), None);
        assert_eq!(palabras("a %"), None);
        assert_eq!(palabras(""), None);
    }

    #[test]
    fn buscar_en_el_tema() {
        let hay = ["/usr/share/icons/hicolor/64x64/apps/foot.png", "/usr/share/icons/hicolor/32x32/apps/foot.png", "/usr/share/icons/hicolor/scalable/apps/gimp.svg", "/usr/share/pixmaps/xterm.png", "/usr/share/icons/Papirus/48x48/apps/foot.png", "/opt/x.png"];
        let existe = |c: &str| hay.contains(&c);
        let bases = ["/home/ana/.local/share/icons", "/usr/share/icons"];
        // Más grande antes que más pequeño; el tema del usuario antes que hicolor.
        assert_eq!(buscar_icono("foot", 48, &[], &bases, &existe), Some(("/usr/share/icons/hicolor/64x64/apps/foot.png".to_string(), Formato::Png)));
        assert_eq!(buscar_icono("foot", 48, &["Papirus"], &bases, &existe).unwrap().0, "/usr/share/icons/Papirus/48x48/apps/foot.png");
        assert_eq!(buscar_icono("foot.png", 48, &[], &bases, &existe).unwrap().0, "/usr/share/icons/hicolor/64x64/apps/foot.png");
        assert_eq!(buscar_icono("gimp", 48, &[], &bases, &existe), Some(("/usr/share/icons/hicolor/scalable/apps/gimp.svg".to_string(), Formato::Svg)));
        assert_eq!(buscar_icono("xterm", 48, &[], &bases, &existe).unwrap().0, "/usr/share/pixmaps/xterm.png");
        assert_eq!(buscar_icono("/opt/x.png", 48, &[], &bases, &existe).unwrap().0, "/opt/x.png");
        // Lo que no está o intenta salirse del tema.
        for malo in ["nada", "../../etc/passwd", "a/b", "..", "", "/etc/passwd", "foo\n"] {
            assert_eq!(buscar_icono(malo, 48, &[], &bases, &existe), None, "{malo}");
        }
    }
}
