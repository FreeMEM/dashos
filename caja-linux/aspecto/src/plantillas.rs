//! Lo que se escribe en la caja Linux con el tema de DashOS (S26): un tema de GTK (3 y 4) hecho aquí desde los
//! papeles de Material 3, los ajustes de GTK, los colores de foot y los envoltorios de las excepciones por programa.
//!
//! Sala limpia: los selectores son los nodos de CSS que documenta GTK (button, entry, check, radio, switch, scale,
//! progressbar, notebook, headerbar, popover...), los colores con nombre son los que documenta libadwaita
//! (`accent_bg_color`, `window_bg_color`... y sus variables `--accent-bg-color`...), y los papeles y estados son los de
//! la especificación de Material 3. Nada copiado de Adwaita ni de otros temas: el tema importa el de GTK que viene en
//! la propia biblioteca (por su recurso) y encima pone lo suyo.

use kit::tema::{Tema, mezcla};

/// La letra de DashOS (la del kit: Roboto, OFL) y la de la terminal (Roboto Mono).
pub const LETRA: &str = "Roboto";
pub const LETRA_FIJA: &str = "Roboto Mono";
/// El nombre del tema de GTK.
pub const TEMA_GTK: &str = "DashOS";

fn hex(c: u32) -> String {
    format!("#{:06x}", c & 0xff_ffff)
}

/// Los colores de un modo, ya en texto de CSS.
struct Papeles {
    p: String,
    sp: String,
    cp: String,
    scp: String,
    s: String,
    ss: String,
    vs: String,
    svs: String,
    contorno: String,
    vc: String,
    sec: String,
    error: String,
    cbajo: String,
    c: String,
    calto: String,
    csec: String,
    /// El botón tonal con la capa de encima (8 %) y de pulsado (10 %), y el relleno igual.
    csec_encima: String,
    csec_pulsado: String,
    p_encima: String,
    p_pulsado: String,
    sobre_error: String,
}

fn papeles(t: &Tema, oscuro: bool) -> Papeles {
    let sobre_error = if oscuro { 0xff60_1410 } else { 0xffff_ffff };
    Papeles {
        p: hex(t.primario),
        sp: hex(t.sobre_primario),
        cp: hex(t.contenedor_primario),
        scp: hex(t.sobre_contenedor_primario),
        s: hex(t.superficie),
        ss: hex(t.sobre_superficie),
        vs: hex(t.variante_superficie),
        svs: hex(t.sobre_variante_superficie),
        contorno: hex(t.contorno),
        vc: hex(t.variante_contorno),
        sec: hex(t.secundario),
        error: hex(t.error),
        cbajo: hex(t.contenedor_bajo()),
        c: hex(t.contenedor()),
        calto: hex(t.contenedor_alto()),
        csec: hex(t.contenedor_secundario()),
        csec_encima: hex(mezcla(t.contenedor_secundario(), t.sobre_superficie, 20)),
        csec_pulsado: hex(mezcla(t.contenedor_secundario(), t.sobre_superficie, 26)),
        p_encima: hex(mezcla(t.primario, t.sobre_primario, 20)),
        p_pulsado: hex(mezcla(t.primario, t.sobre_primario, 26)),
        sobre_error: hex(sobre_error),
    }
}

/// Los colores con nombre (los de libadwaita y los clásicos de GTK 3) y, en GTK 4, sus variables de CSS (GTK 4.16 y
/// siguientes): para los programas que los usan.
fn colores_con_nombre(k: &Papeles, version: u32) -> String {
    let mut t = String::new();
    // Los colores con nombre (los de libadwaita y los clásicos de GTK 3), para los programas que los usan.
    let nombres: [(&str, &String); 33] = [
        ("accent_color", &k.p),
        ("accent_bg_color", &k.p),
        ("accent_fg_color", &k.sp),
        ("destructive_color", &k.error),
        ("destructive_bg_color", &k.error),
        ("destructive_fg_color", &k.sobre_error),
        ("error_color", &k.error),
        ("window_bg_color", &k.s),
        ("window_fg_color", &k.ss),
        ("view_bg_color", &k.cbajo),
        ("view_fg_color", &k.ss),
        ("headerbar_bg_color", &k.c),
        ("headerbar_fg_color", &k.ss),
        ("headerbar_border_color", &k.vc),
        ("headerbar_backdrop_color", &k.s),
        ("sidebar_bg_color", &k.cbajo),
        ("sidebar_fg_color", &k.ss),
        ("card_bg_color", &k.cbajo),
        ("card_fg_color", &k.ss),
        ("dialog_bg_color", &k.calto),
        ("dialog_fg_color", &k.ss),
        ("popover_bg_color", &k.calto),
        ("popover_fg_color", &k.ss),
        ("theme_bg_color", &k.s),
        ("theme_fg_color", &k.ss),
        ("theme_base_color", &k.cbajo),
        ("theme_text_color", &k.ss),
        ("theme_selected_bg_color", &k.p),
        ("theme_selected_fg_color", &k.sp),
        ("insensitive_fg_color", &k.svs),
        ("borders", &k.vc),
        ("unfocused_borders", &k.vc),
        ("link_color", &k.p),
    ];
    for (n, c) in nombres {
        t.push_str(&format!("@define-color {} {};\n", n, c));
    }
    if version >= 4 {
        // Las variables de CSS (GTK 4.16 y siguientes; libadwaita las usa).
        t.push_str(&format!(
            "\n:root {{\n  --accent-color: {p};\n  --accent-bg-color: {p};\n  --accent-fg-color: {sp};\n  --window-bg-color: {s};\n  \
             --window-fg-color: {ss};\n  --view-bg-color: {cb};\n  --view-fg-color: {ss};\n  --headerbar-bg-color: {c};\n  \
             --headerbar-fg-color: {ss};\n  --popover-bg-color: {ca};\n  --popover-fg-color: {ss};\n  --dialog-bg-color: {ca};\n  \
             --card-bg-color: {cb};\n  --sidebar-bg-color: {cb};\n  --border-color: {vc};\n  --destructive-bg-color: {e};\n  \
             --window-radius: 12px;\n}}\n",
            p = k.p, sp = k.sp, s = k.s, ss = k.ss, cb = k.cbajo, c = k.c, ca = k.calto, vc = k.vc, e = k.error
        ));
    }
    t
}

/// `~/.config/gtk-4.0/gtk.css`: los colores con nombre y las variables, solos. libadwaita no usa temas de GTK pero sí
/// lee este fichero (y sus colores salen de estas variables); a un programa de GTK 4 sin libadwaita no le cambia nada.
pub fn libadwaita_css(t: &Tema, oscuro: bool) -> String {
    format!(
        "/* Lo escribe dashos-aspecto (S26): los colores de DashOS para los programas de libadwaita ({}). Se rehace cada vez\n   que cambia el tema de DashOS. */\n{}",
        if oscuro { "oscuro" } else { "claro" },
        colores_con_nombre(&papeles(t, oscuro), 4)
    )
}

/// El CSS del tema de DashOS para GTK `version` (3 o 4), en el modo `oscuro`, con el esquema `t`.
pub fn gtk_css(t: &Tema, oscuro: bool, version: u32, acento: u32) -> String {
    let k = papeles(t, oscuro);
    let base = match (version, oscuro) {
        (3, false) => "resource:///org/gtk/libgtk/theme/Adwaita/gtk-contained.css",
        (3, true) => "resource:///org/gtk/libgtk/theme/Adwaita/gtk-contained-dark.css",
        (_, false) => "resource:///org/gtk/libgtk/theme/Default/Default-light.css",
        (_, true) => "resource:///org/gtk/libgtk/theme/Default/Default-dark.css",
    };
    let mut css = format!(
        "/* El tema de DashOS para GTK {version} (S26), {modo}, acento {acento}. Lo escribe dashos-aspecto con los colores del\n   \
         sistema: no se toca a mano (se rehace cada vez que cambia el tema de DashOS). */\n\
         @import url(\"{base}\");\n\n",
        modo = if oscuro { "oscuro" } else { "claro" },
        acento = hex(acento),
    );
    css.push_str(&colores_con_nombre(&k, version));
    // Los controles, con los papeles de Material 3. Las capas de estado: encima 8 %, pulsado 10 %; desactivado, el
    // contenido al 38 % y el fondo al 12 %.
    css.push_str("\n/* Los controles. Cada selector lleva :not(#dashos): con ese peso de identificador gana a las reglas del tema de\n   GTK de debajo (sus estados, :backdrop...), que no usan identificadores; GTK no tiene !important. */\n");
    let foco = if version >= 4 { ":focus-within" } else { ":focus" };
    let reglas: Vec<(String, String)> = vec![
        // La ventana y la letra.
        ("window, .background, dialog, messagedialog, popover, menu, tooltip".into(), format!("font-family: \"{}\", sans-serif;", LETRA)),
        ("window, window.background, .background".into(), format!("background-color: {}; color: {};", k.s, k.ss)),
        ("window.csd, window.csd decoration".into(), "border-radius: 12px;".into()),
        ("*".into(), format!("outline-color: {};", k.sec)),
        ("separator".into(), format!("background-color: {};", k.vc)),
        ("label.dim-label, .dim-label".into(), format!("color: {}; opacity: 1;", k.svs)),
        ("link, *:link, button.link".into(), format!("color: {};", k.p)),
        // La barra de título (Material 3: barra superior, sobre el contenedor de superficie).
        ("headerbar, .titlebar".into(), format!("background-color: {}; color: {}; border-bottom: 1px solid {}; box-shadow: none;", k.c, k.ss, k.vc)),
        ("headerbar:backdrop, .titlebar:backdrop".into(), format!("background-color: {}; color: {};", k.s, k.svs)),
        ("headerbar .title, headerbar label.title".into(), "font-weight: 500;".into()),
        // Botones: tonal (los normales), relleno (la acción principal), de error, y de texto en las barras.
        ("button".into(), format!("border-radius: 9999px; background-color: {}; color: {}; border-color: transparent; box-shadow: none; text-shadow: none; -gtk-icon-shadow: none; font-weight: 500;", k.csec, k.ss)),
        ("button:hover".into(), format!("background-color: {};", k.csec_encima)),
        ("button:active".into(), format!("background-color: {};", k.csec_pulsado)),
        ("button:checked".into(), format!("background-color: {}; color: {};", k.cp, k.scp)),
        ("button:disabled".into(), format!("background-color: alpha({ss}, 0.12); color: alpha({ss}, 0.38);", ss = k.ss)),
        ("button.suggested-action, button.default".into(), format!("background-color: {}; color: {};", k.p, k.sp)),
        ("button.suggested-action:hover, button.default:hover".into(), format!("background-color: {};", k.p_encima)),
        ("button.suggested-action:active, button.default:active".into(), format!("background-color: {};", k.p_pulsado)),
        ("button.destructive-action".into(), format!("background-color: {}; color: {};", k.error, k.sobre_error)),
        ("button.flat, headerbar button, .titlebar button, spinbutton button, notebook tab button, treeview.view header button, popover modelbutton, menuitem".into(),
            format!("background-color: transparent; color: {}; border-color: transparent;", k.svs)),
        ("treeview.view header button".into(), format!("border-radius: 0; border-bottom: 1px solid {}; font-weight: normal;", k.vc)),
        ("spinbutton button, notebook tab button".into(), "border-radius: 9999px;".into()),
        ("button.flat:hover, headerbar button:hover, .titlebar button:hover, spinbutton button:hover, notebook tab button:hover, popover modelbutton:hover, menuitem:hover".into(),
            format!("background-color: alpha({}, 0.08);", k.ss)),
        ("button.flat:active, headerbar button:active, spinbutton button:active".into(), format!("background-color: alpha({}, 0.10);", k.ss)),
        ("headerbar button.suggested-action".into(), format!("background-color: {}; color: {};", k.p, k.sp)),
        ("headerbar button.destructive-action".into(), format!("background-color: {}; color: {};", k.error, k.sobre_error)),
        // Campos de texto (Material 3: con contorno; con el foco, el contorno del primario y más grueso).
        ("entry, spinbutton:not(.vertical)".into(), format!("border-radius: 4px; background-color: {}; color: {}; border: 1px solid {}; box-shadow: none; caret-color: {};", k.cbajo, k.ss, k.contorno, k.p)),
        ("spinbutton entry, spinbutton text".into(), "border: none; box-shadow: none; background-color: transparent;".into()),
        (format!("entry{f}, spinbutton{f}", f = foco), format!("border-color: {p}; box-shadow: inset 0 0 0 1px {p}; outline: none;", p = k.p)),
        ("entry:disabled".into(), format!("color: alpha({ss}, 0.38); border-color: alpha({ss}, 0.12);", ss = k.ss)),
        ("entry selection, text selection, label selection, selection".into(), format!("background-color: alpha({}, 0.30); color: {};", k.p, k.ss)),
        // Casillas, opciones e interruptores.
        ("check, radio".into(), format!("background-color: transparent; border: 2px solid {}; color: {}; box-shadow: none; -gtk-icon-shadow: none;", k.svs, k.sp)),
        ("check".into(), "border-radius: 2px;".into()),
        ("radio".into(), "border-radius: 9999px;".into()),
        ("check:checked, check:indeterminate".into(), format!("background-color: {p}; border-color: {p}; color: {sp};", p = k.p, sp = k.sp)),
        ("radio:checked".into(), format!("background-color: transparent; border-color: {p}; color: {p};", p = k.p)),
        ("switch".into(), format!("background-color: {}; border: 2px solid {}; border-radius: 9999px; box-shadow: none; color: transparent;", k.vs, k.contorno)),
        ("switch:checked".into(), format!("background-color: {p}; border-color: {p};", p = k.p)),
        ("switch slider".into(), format!("background-color: {}; border: 4px solid transparent; border-radius: 9999px; box-shadow: none;", k.contorno)),
        ("switch:checked slider".into(), format!("background-color: {}; border-width: 2px;", k.sp)),
        // Deslizadores y barras de progreso.
        ("scale trough, progressbar trough".into(), format!("background-color: {}; border: none; border-radius: 9999px; box-shadow: none;", k.vs)),
        ("scale highlight, progressbar progress".into(), format!("background-color: {}; border: none; border-radius: 9999px; box-shadow: none; margin: 0;", k.p)),
        ("scale.horizontal trough, scale.horizontal highlight, progressbar.horizontal trough, progressbar.horizontal progress".into(), "min-height: 4px;".into()),
        ("scale.vertical trough, scale.vertical highlight, progressbar.vertical trough, progressbar.vertical progress".into(), "min-width: 4px;".into()),
        ("scale slider".into(), format!("background-color: {}; border: none; border-radius: 9999px; box-shadow: none;", k.p)),
        // Pestañas (Material 3: el texto de la elegida y una raya del primario).
        ("notebook, notebook > header, notebook > stack".into(), format!("background-color: {}; border-color: {};", k.s, k.vc)),
        ("notebook > header tab".into(), format!("color: {}; box-shadow: none; border-color: transparent; background-color: transparent;", k.svs)),
        ("notebook > header tab:hover".into(), format!("background-color: alpha({}, 0.08);", k.ss)),
        ("notebook > header tab:checked".into(), format!("color: {p}; box-shadow: inset 0 -3px {p};", p = k.p)),
        // Listas y vistas.
        ("list, listview, treeview.view, textview text, iconview, columnview, .view".into(), format!("background-color: {}; color: {};", k.cbajo, k.ss)),
        ("row:hover".into(), format!("background-color: alpha({}, 0.08);", k.ss)),
        ("row:selected, treeview.view:selected, .view:selected, row:selected:hover".into(), format!("background-color: {}; color: {};", k.csec, k.ss)),
        ("frame > border, scrolledwindow.frame, .frame".into(), format!("border-color: {};", k.vc)),
        // Menús, emergentes y avisos.
        ("menu, .menu, .context-menu, popover > contents, popover.background > contents, popover.menu > contents".into(), format!("background-color: {}; color: {}; border-radius: 8px;", k.calto, k.ss)),
        ("tooltip, tooltip.background".into(), format!("background-color: {}; color: {}; border-radius: 4px;", k.ss, k.s)),
        ("tooltip *".into(), format!("color: {};", k.s)),
    ];
    for (selectores, cuerpo) in reglas {
        regla(&mut css, &selectores, &cuerpo);
    }
    css
}

/// Una regla con su peso (`:not(#dashos)` en cada selector) y, si pone un color de fondo, sin la imagen de fondo de
/// debajo (los degradados del tema de GTK).
fn regla(css: &mut String, selectores: &str, cuerpo: &str) {
    let s: Vec<String> = selectores.split(", ").map(|x| format!("{}:not(#dashos)", x)).collect();
    let mut c = String::from(cuerpo);
    if c.contains("background-color") && !c.contains("background-image") {
        c.insert_str(0, "background-image: none; ");
    }
    css.push_str(&format!("{} {{ {} }}\n", s.join(", "), c));
}

/// `settings.ini` de GTK: el tema (`tema`: el de DashOS o el de GTK, Adwaita), el modo y, con DashOS, la letra.
pub fn gtk_ajustes(tema: &str, dashos: bool, oscuro: bool) -> String {
    let mut t = String::from("# Lo escribe dashos-aspecto (S26): se rehace cada vez que cambia el tema de DashOS.\n[Settings]\n");
    t.push_str(&format!("gtk-theme-name={}\n", tema));
    // El modo es del sistema: también con el aspecto original (como el color-scheme del portal de freedesktop).
    t.push_str(&format!("gtk-application-prefer-dark-theme={}\n", oscuro as u32));
    if dashos {
        t.push_str(&format!("gtk-font-name={} 11\n", LETRA));
    }
    t
}

/// El acento de GNOME más cercano a un color (los nueve de `org.gnome.desktop.interface accent-color`, por su matiz;
/// uno casi gris, "slate"): lo que leen libadwaita y el portal.
pub fn acento_gnome(c: u32) -> &'static str {
    let (r, g, b) = (((c >> 16) & 0xff) as f64 / 255.0, ((c >> 8) & 0xff) as f64 / 255.0, (c & 0xff) as f64 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max - min < 0.12 {
        return "slate";
    }
    let d = max - min;
    let h = if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    const MATICES: [(&str, f64); 9] =
        [("red", 0.0), ("orange", 30.0), ("yellow", 50.0), ("green", 120.0), ("teal", 180.0), ("blue", 215.0), ("purple", 270.0), ("pink", 330.0), ("red", 360.0)];
    MATICES.iter().min_by(|a, b| (a.1 - h).abs().total_cmp(&(b.1 - h).abs())).unwrap().0
}

/// El fichero de GSettings (`keyfile`) con lo de `org.gnome.desktop.interface`: el tema, el modo, el acento y, con
/// DashOS, las letras.
pub fn gsettings(tema: &str, dashos: bool, oscuro: bool, primario: u32) -> String {
    let mut t = format!(
        "# Lo escribe dashos-aspecto (S26): se rehace cada vez que cambia el tema de DashOS.\n[org/gnome/desktop/interface]\n\
         gtk-theme='{}'\ncolor-scheme='{}'\naccent-color='{}'\n",
        tema,
        if oscuro { "prefer-dark" } else { "prefer-light" },
        acento_gnome(primario)
    );
    if dashos {
        t.push_str(&format!("font-name='{} 11'\nmonospace-font-name='{} 11'\n", LETRA, LETRA_FIJA));
    }
    t
}

/// La configuración de foot con los colores de DashOS (los dos modos; empieza en el del sistema) y Roboto Mono.
pub fn foot_ini(claro: &Tema, oscuro_t: &Tema, oscuro: bool) -> String {
    let colores = |t: &Tema| {
        format!(
            "background={}\nforeground={}\nselection-background={}\nselection-foreground={}\nurls={}\ncursor={} {}\n",
            &hex(t.superficie)[1..],
            &hex(t.sobre_superficie)[1..],
            &hex(t.contenedor_primario)[1..],
            &hex(t.sobre_contenedor_primario)[1..],
            &hex(t.primario)[1..],
            &hex(t.superficie)[1..],
            &hex(t.primario)[1..],
        )
    };
    format!(
        "# Lo escribe dashos-aspecto (S26): los colores y la letra de DashOS. Se rehace cada vez que cambia el tema.\n\
         [main]\ninclude=/etc/xdg/foot/foot.ini\nfont={}:size=11\ninitial-color-theme={}\n\n[colors-dark]\n{}\n[colors-light]\n{}",
        LETRA_FIJA,
        if oscuro { "dark" } else { "light" },
        colores(oscuro_t),
        colores(claro)
    )
}

/// Un nombre de programa de Linux que se puede envolver: solo letras, cifras y `._+-` (va en una ruta y en un guion de
/// shell), y que no empieza por `-` ni es `.` o `..`.
pub fn envolvible(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && !n.starts_with('-')
        && n != "."
        && n != ".."
        && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
}

/// El envoltorio de un programa con un aspecto distinto del general: pone el tema de GTK y lo de Qt y ejecuta el de
/// verdad (el siguiente del PATH, sin la carpeta de los envoltorios).
pub fn envoltorio(programa: &str, dashos: bool, oscuro: bool, carpeta: &str) -> String {
    let tema = if dashos { TEMA_GTK } else { "Adwaita" };
    let variante = if oscuro { ":dark" } else { "" };
    let qt = if dashos { "export QT_QPA_PLATFORMTHEME=gtk3" } else { "unset QT_QPA_PLATFORMTHEME" };
    format!(
        "#!/bin/sh\n# Lo escribe dashos-aspecto (S26): {programa}, {que}.\nexport GTK_THEME={tema}{variante}\n{qt}\n\
         PATH=$(printf '%s' \"$PATH\" | tr ':' '\\n' | grep -vx '{carpeta}' | paste -sd: -)\nexport PATH\nexec {programa} \"$@\"\n",
        que = if dashos { "con el aspecto de DashOS" } else { "con su aspecto original" },
    )
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn css_con_los_colores_del_tema() {
        for oscuro in [false, true] {
            let t = Tema::del_sistema(0xff00_61a4, oscuro);
            for v in [3, 4] {
                let css = gtk_css(&t, oscuro, v, 0xff00_61a4);
                assert!(css.contains(&format!("@define-color accent_bg_color {};", hex(t.primario))));
                assert!(css.contains(&format!("window:not(#dashos), window.background:not(#dashos), .background:not(#dashos) {{ background-image: none; background-color: {};", hex(t.superficie))));
                assert!(css.contains(if v == 3 { "Adwaita/gtk-contained" } else { "Default/Default-" }));
                assert_eq!(css.contains(":root"), v == 4);
                // Llaves equilibradas: un CSS que GTK lee entero.
                assert_eq!(css.matches('{').count(), css.matches('}').count());
            }
        }
        assert!(gtk_css(&Tema::OSCURO, true, 3, 0xff67_50a4).contains("gtk-contained-dark.css"));
    }

    #[test]
    fn ajustes_y_foot() {
        assert_eq!(gtk_ajustes("Adwaita", false, true).lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>(), ["[Settings]", "gtk-theme-name=Adwaita", "gtk-application-prefer-dark-theme=1"]);
        assert!(gtk_ajustes("DashOS-1", true, false).contains("gtk-theme-name=DashOS-1\n"));
        assert_eq!([0xff0061a4, 0xff386a20, 0xff8b5000, 0xff6750a4, 0xffb3261e, 0xff777777].map(acento_gnome), ["blue", "green", "orange", "purple", "red", "slate"]);
        let g = gsettings("DashOS-2", true, true, 0xffd0bcff);
        assert!(g.contains("gtk-theme='DashOS-2'") && g.contains("color-scheme='prefer-dark'") && g.contains("font-name='Roboto 11'"));
        let f = foot_ini(&Tema::CLARO, &Tema::OSCURO, true);
        assert!(f.contains("initial-color-theme=dark") && f.contains("[colors-dark]\nbackground=141218\n"));
    }

    #[test]
    fn envoltorios() {
        for bien in ["gimp", "gtk4-widget-factory", "org.gnome.Calculator", "c++filt"] {
            assert!(envolvible(bien), "{}", bien);
        }
        for mal in ["", "-rf", "..", ".", "a b", "a/b", "$(reboot)", "a;b", "a\nb", "ñ", &"x".repeat(65)] {
            assert!(!envolvible(mal), "{:?}", mal);
        }
        let e = envoltorio("gimp", false, true, "/run/dashos-aspecto/bin");
        assert!(e.contains("GTK_THEME=Adwaita:dark") && e.contains("unset QT_QPA_PLATFORMTHEME") && e.ends_with("exec gimp \"$@\"\n"));
    }
}
