//! `dashos-aspecto` (S26, `docs/diseño/S26-aspecto-de-las-cajas.md`): el agente del aspecto dentro de la caja Linux.
//!
//! Se conecta al compositor de DashOS como un programa de Wayland más (por el puente de la caja, `/run/wayland-0`),
//! enlaza el tema del sistema (`dashos_tema_v1`, versión 3) y, cada vez que cambia (color de acento o paleta del fondo,
//! modo claro u oscuro, o el aspecto de los programas de las cajas), escribe:
//!
//! - el tema de GTK "DashOS" (GTK 3 y 4, claro y oscuro) en `~/.local/share/themes/DashOS/`;
//! - `~/.config/gtk-3.0/settings.ini` y `gtk-4.0/settings.ini`: el tema (DashOS, o Adwaita con el aspecto original),
//!   el modo y la letra;
//! - `~/.config/foot/foot.ini` con los colores de DashOS (con el aspecto original, se quita);
//! - en `/run/dashos-aspecto/`: `linux` ("dashos" u "original", que mira `/etc/profile.d/dashos-aspecto.sh` para
//!   poner Qt con los colores de GTK 3) y `bin/`, un envoltorio por cada programa con un aspecto distinto del de la caja.
//!
//! Lo que ya está abierto: GTK 3 cambia en el acto (GSettings con el fichero, que GLib vigila, y un nombre de tema
//! nuevo cada vez); GTK 4 sin portal de ajustes, Qt y foot, al abrirse otra vez (ver el estudio: falta el portal).
//!
//! Uso:
//!   dashos-aspecto [SOCKET]                       el servicio (sin SOCKET, $WAYLAND_DISPLAY)
//!   dashos-aspecto --poner RRGGBB claro|oscuro [AJUSTE]   escribe una vez, sin Wayland (pruebas en el PC)
//! Con DASHOS_ASPECTO_RUN, otra carpeta en vez de /run/dashos-aspecto; con HOME y XDG_*, otras carpetas de usuario.

mod plantillas;

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use kit::tema::Tema;
use wayland::aspecto::{Ajuste, Aspecto, Caja};
use wayland::tablas::{dashos_tema_v1, wl_callback, wl_display, wl_registry};
use wayland::Escritor;

const REGISTRO: u32 = 2;
const SYNC: u32 = 3;
const TEMA: u32 = 4;

/// Lo que se sabe del tema de DashOS.
#[derive(Clone, PartialEq)]
struct Estado {
    color: u32,
    oscuro: bool,
    ajuste: Ajuste,
}

fn decir(t: &str) {
    println!("{}", t);
    // En la caja, también por la consola (hvc0): sale en el registro de DashOS.
    if let Ok(mut f) = fs::OpenOptions::new().write(true).open("/dev/hvc0") {
        let _ = writeln!(f, "{}", t);
    }
}

fn carpeta(var: &str, por_defecto: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/root".into())).join(por_defecto),
    }
}

/// Escribe `datos` en `ruta` de una vez (un fichero aparte y `rename`): quien lo lee nunca ve uno a medias.
fn escribir(ruta: &Path, datos: &str, modo_ejecutable: bool) -> std::io::Result<()> {
    if let Some(p) = ruta.parent() {
        fs::create_dir_all(p)?;
    }
    let tmp = ruta.with_extension("dashos-tmp");
    fs::write(&tmp, datos)?;
    if modo_ejecutable {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(&tmp, ruta)
}

/// Quita un fichero si lo escribió este agente (su primera o segunda línea lo dice): uno del usuario no se toca.
fn quitar_si_es_mio(ruta: &Path) {
    if fs::read_to_string(ruta).is_ok_and(|t| t.lines().take(2).any(|l| l.contains("dashos-aspecto"))) {
        let _ = fs::remove_file(ruta);
    }
}

/// Pone en la caja lo que dice el tema de DashOS.
fn aplicar(e: &Estado) -> std::io::Result<String> {
    let claro = Tema::del_sistema(e.color, false);
    let oscuro = Tema::del_sistema(e.color, true);
    let linux = e.ajuste.de_caja(Caja::Linux);
    let dashos = linux == Aspecto::Dashos;
    let config = carpeta("XDG_CONFIG_HOME", ".config");
    let datos = carpeta("XDG_DATA_HOME", ".local/share");
    // El tema de GTK, siempre (los programas con excepción "dashos" lo usan aunque la caja vaya con el original): en
    // "DashOS" (el de los envoltorios) y en "DashOS-1" o "DashOS-2", uno cada vez. GTK solo vuelve a leer un tema si
    // cambia su nombre: así, al cambiar el nombre en los ajustes de GSettings, los programas abiertos lo cambian.
    let otro = if fs::read_to_string(config.join("glib-2.0/settings/keyfile")).is_ok_and(|t| t.contains("'DashOS-1'")) { 2 } else { 1 };
    let nombre_vivo = format!("{}-{}", plantillas::TEMA_GTK, otro);
    // En el de los envoltorios, gtk.css es el claro y gtk-dark.css el oscuro (GTK_THEME=DashOS:dark); en el vivo, los dos
    // son los del modo de ahora: GTK 3 cambia de tema en el acto, pero no mira otra vez `prefer-dark` de settings.ini.
    for nombre in [plantillas::TEMA_GTK, nombre_vivo.as_str()] {
        let tema = datos.join("themes").join(nombre);
        let vivo = nombre != plantillas::TEMA_GTK;
        for v in [3u32, 4] {
            let d = tema.join(format!("gtk-{}.0", v));
            let (ligero, oscuro_css) = (plantillas::gtk_css(&claro, false, v, e.color), plantillas::gtk_css(&oscuro, true, v, e.color));
            let ahora = if e.oscuro { &oscuro_css } else { &ligero };
            escribir(&d.join("gtk.css"), if vivo { ahora } else { &ligero }, false)?;
            escribir(&d.join("gtk-dark.css"), if vivo { ahora } else { &oscuro_css }, false)?;
        }
        escribir(&tema.join("index.theme"), &format!("[Desktop Entry]\nType=X-GNOME-Metatheme\nName={0}\nComment=El tema de DashOS (S26), de dashos-aspecto\n\n[X-GNOME-Metatheme]\nGtkTheme={0}\n", nombre), false)?;
    }
    let nombre_gtk = if dashos { nombre_vivo.as_str() } else { "Adwaita" };
    for v in [3u32, 4] {
        escribir(&config.join(format!("gtk-{}.0", v)).join("settings.ini"), &plantillas::gtk_ajustes(nombre_gtk, dashos, e.oscuro), false)?;
    }
    // GSettings (con GSETTINGS_BACKEND=keyfile, que pone /etc/profile.d/dashos-aspecto.sh): en Wayland, GTK 3 y 4
    // toman de aquí el tema, la letra y el modo (por encima de settings.ini), y miran si cambia el fichero.
    let primario = if e.oscuro { oscuro.primario } else { claro.primario };
    escribir(&config.join("glib-2.0/settings/keyfile"), &plantillas::gsettings(nombre_gtk, dashos, e.oscuro, primario), false)?;
    // libadwaita no usa temas: sus colores, por el CSS del usuario de GTK 4 (solo colores y variables).
    let usuario4 = config.join("gtk-4.0").join("gtk.css");
    if dashos {
        escribir(&usuario4, &plantillas::libadwaita_css(if e.oscuro { &oscuro } else { &claro }, e.oscuro), false)?;
    } else {
        quitar_si_es_mio(&usuario4);
    }
    let foot = config.join("foot").join("foot.ini");
    if dashos {
        escribir(&foot, &plantillas::foot_ini(&claro, &oscuro, e.oscuro), false)?;
    } else {
        quitar_si_es_mio(&foot);
    }
    // Lo de todo el sistema: el aspecto de la caja (para Qt) y los envoltorios de las excepciones.
    let run = PathBuf::from(std::env::var_os("DASHOS_ASPECTO_RUN").unwrap_or_else(|| "/run/dashos-aspecto".into()));
    escribir(&run.join("linux"), &format!("{}\n", linux.nombre()), false)?;
    let bin = run.join("bin");
    fs::create_dir_all(&bin)?;
    let mut envueltos = Vec::new();
    let mut saltados = 0;
    for (caja, nombre, aspecto) in &e.ajuste.programas {
        if *caja != Caja::Linux || *aspecto == linux {
            continue;
        }
        if !plantillas::envolvible(nombre) {
            saltados += 1;
            continue;
        }
        let texto = plantillas::envoltorio(nombre, *aspecto == Aspecto::Dashos, e.oscuro, &bin.to_string_lossy());
        escribir(&bin.join(nombre), &texto, true)?;
        envueltos.push(nombre.clone());
    }
    for f in fs::read_dir(&bin)?.flatten() {
        let n = f.file_name().to_string_lossy().into_owned();
        if !envueltos.contains(&n) {
            quitar_si_es_mio(&f.path());
        }
    }
    Ok(format!(
        "aspecto: programas de Linux {}, {}, acento {:06x} (superficie {:06x}, primario {:06x}); {} excepciones{}; GTK 3 y 4, foot y Qt al día",
        if dashos { "como DashOS" } else { "originales" },
        if e.oscuro { "oscuro" } else { "claro" },
        e.color & 0xff_ffff,
        (if e.oscuro { oscuro.superficie } else { claro.superficie }) & 0xff_ffff,
        (if e.oscuro { oscuro.primario } else { claro.primario }) & 0xff_ffff,
        envueltos.len(),
        if saltados > 0 { format!(" ({} con un nombre que no se puede envolver)", saltados) } else { String::new() },
    ))
}

/// Lee mensajes de Wayland de `s` hasta tener al menos uno entero; devuelve los que haya.
fn leer_mensajes(s: &mut UnixStream, buf: &mut Vec<u8>) -> std::io::Result<Vec<(u32, u16, Vec<u8>)>> {
    let mut out = Vec::new();
    loop {
        while let Ok((objeto, orden, tam)) = wayland::leer_cabecera(buf) {
            out.push((objeto, orden, buf[8..tam].to_vec()));
            buf.drain(..tam);
        }
        if !out.is_empty() {
            return Ok(out);
        }
        // Una cabecera rota (no un mensaje a medias) no se arregla leyendo más.
        if let Err(wayland::Fallo::Tamano) = wayland::leer_cabecera(buf) {
            return Err(std::io::Error::other("mensaje de Wayland roto"));
        }
        let mut trozo = [0u8; 8192];
        let n = s.read(&mut trozo)?;
        if n == 0 {
            return Err(std::io::Error::other("el compositor cerró la conexión"));
        }
        buf.extend_from_slice(&trozo[..n]);
        if buf.len() > 1 << 20 {
            return Err(std::io::Error::other("demasiado sin leer"));
        }
    }
}

fn texto_de(cuerpo: &[u8]) -> Option<&str> {
    let n = u32::from_le_bytes(cuerpo.get(..4)?.try_into().ok()?) as usize;
    let b = cuerpo.get(4..4 + n)?;
    core::str::from_utf8(b.strip_suffix(&[0])?).ok()
}

fn servicio(ruta: &Path) -> std::io::Result<()> {
    let mut s = UnixStream::connect(ruta)?;
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(REGISTRO).fin();
    e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
    s.write_all(&e.bytes)?;
    let mut buf = Vec::new();
    let mut global = None;
    'globales: loop {
        for (o, orden, c) in leer_mensajes(&mut s, &mut buf)? {
            if o == REGISTRO && orden == wl_registry::ev::GLOBAL && texto_de(&c[4..]) == Some("dashos_tema_v1") {
                let nombre = u32::from_le_bytes(c[..4].try_into().unwrap());
                let n = u32::from_le_bytes(c[4..8].try_into().unwrap()) as usize;
                let v = c.get(8 + n.div_ceil(4) * 4..).and_then(|r| r.get(..4)).map_or(1, |r| u32::from_le_bytes(r.try_into().unwrap()));
                global = Some((nombre, v.min(3)));
            }
            if o == SYNC && orden == wl_callback::ev::DONE {
                break 'globales;
            }
        }
    }
    let Some((nombre, version)) = global else {
        decir("aspecto: el compositor no tiene el tema de DashOS (dashos_tema_v1): nada que hacer");
        return Ok(());
    };
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(nombre).texto(Some("dashos_tema_v1")).uint(version).uint(TEMA).fin();
    s.write_all(&e.bytes)?;
    if version < 3 {
        decir("aspecto: el compositor no dice el aspecto de las cajas (dashos_tema_v1 < 3): como DashOS");
    }
    let mut ahora: Option<Estado> = None;
    let (mut color, mut oscuro, mut ajuste) = (None, false, if version < 3 { Some(Ajuste::DE_SERIE) } else { None });
    loop {
        for (o, orden, c) in leer_mensajes(&mut s, &mut buf)? {
            match (o, orden) {
                (TEMA, dashos_tema_v1::ev::ESQUEMA) => {
                    kit::tema::recordar_evento_esquema(&c);
                }
                (TEMA, dashos_tema_v1::ev::TEMA) if c.len() >= 8 => {
                    color = Some(u32::from_le_bytes(c[..4].try_into().unwrap()) | 0xff00_0000);
                    oscuro = u32::from_le_bytes(c[4..8].try_into().unwrap()) != 0;
                }
                (TEMA, dashos_tema_v1::ev::CAJAS) => match texto_de(&c).map(|t| Ajuste::entender(t.as_bytes())) {
                    Some(Ok(a)) => ajuste = Some(a),
                    _ => decir("aspecto: un aspecto de las cajas que no se entiende; se queda el de antes"),
                },
                (1, wl_display::ev::ERROR) => return Err(std::io::Error::other("error de protocolo del compositor")),
                _ => {}
            }
        }
        // Con todo lo que llegó junto ya leído: si cambió algo, se aplica.
        if let (Some(color), Some(ajuste)) = (color, ajuste.clone()) {
            let nuevo = Estado { color, oscuro, ajuste };
            if ahora.as_ref() != Some(&nuevo) {
                match aplicar(&nuevo) {
                    Ok(t) => decir(&t),
                    Err(err) => decir(&format!("aspecto: no se pudo escribir ({})", err)),
                }
                ahora = Some(nuevo);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--poner") {
        let color = args.get(2).and_then(|c| u32::from_str_radix(c, 16).ok()).unwrap_or(0x6750a4) | 0xff00_0000;
        let oscuro = args.get(3).map(String::as_str) == Some("oscuro");
        let ajuste = match args.get(4) {
            Some(t) => match Ajuste::entender(t.replace("\\n", "\n").as_bytes()) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("dashos-aspecto: el ajuste no vale ({:?})", e);
                    std::process::exit(2);
                }
            },
            None => Ajuste::DE_SERIE,
        };
        match aplicar(&Estado { color, oscuro, ajuste }) {
            Ok(t) => println!("{}", t),
            Err(e) => {
                eprintln!("dashos-aspecto: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }
    let ruta = match args.get(1) {
        Some(r) => PathBuf::from(r),
        None => {
            let d = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
            if d.starts_with('/') {
                PathBuf::from(d)
            } else {
                PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run".into())).join(d)
            }
        }
    };
    if let Err(e) = servicio(&ruta) {
        decir(&format!("aspecto: {} ({})", e, ruta.display()));
        std::process::exit(1);
    }
}
