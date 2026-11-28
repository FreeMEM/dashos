//! ramlib en la caja: lo que exec pide cuando un `OpenLibrary` u `OpenDevice` no encuentra lo que busca en memoria
//! (`caja_exec::ramlib`). Se busca el fichero con dos (las asignaciones de la caja: `LIBS:`, `DEVS:`, `Classes:` y el
//! cajón del programa) y se carga con el cargador hunk de dos, en memoria del sistema (la biblioteca sigue cuando la
//! tarea que la abrió acaba, hasta que se echa). Exec busca después su `RomTag` y la inicia.
//!
//! Nunca de disco, aunque estén: exec, graphics, layers, intuition y dos (las de la caja son nativas, y las de un
//! AmigaOS hablan con el chipset). Lo nativo gana también para todas las demás: exec solo pide lo que no está.

use alloc::format;
use alloc::vec::Vec;

use caja_exec::Memoria;

use super::{Caja, CajaDos, di};

/// Las que no se cargan nunca de disco: las del sistema (nativas en la caja; las de un AmigaOS hablan con el
/// chipset) y las que solo sirven para el hardware del Amiga (lo pedirían y fallarían a medias, o colgarían).
const RECHAZADAS: [&[u8]; 22] = [
    b"exec.library",
    b"graphics.library",
    b"layers.library",
    b"intuition.library",
    b"dos.library",
    b"lowlevel.library",
    b"68020.library",
    b"68030.library",
    b"68040.library",
    b"68060.library",
    b"680x0.library",
    b"mmu.library",
    b"audio.device",
    b"trackdisk.device",
    b"gameport.device",
    b"keyboard.device",
    b"input.device",
    b"serial.device",
    b"parallel.device",
    b"scsi.device",
    b"carddisk.device",
    b"printer.device",
];

/// Las de un AmigaOS de verdad que aún no van en la caja: su `Init` llama a funciones privadas de exec (sin
/// documentar), y el programa que las abriera acabaría ahí. Mejor que no abran: muchos programas siguen sin ellas
/// (MUI, sin icon.library). Lo que hace falta son nativas. Vistas en el AmigaOS 3.2 (la caja lo dice: "falta:
/// execPrivate17"). icon, workbench, asl y commodities ya son nativas: lo nativo gana y estas no se piden; si un programa
/// pide una por su camino (LIBS:asl.library), tampoco se carga la de disco.
const SIN_SOPORTE: [&[u8]; 2] = [b"asl.library", b"commodities.library"];

/// Dónde dice el usuario que está su AmigaOS (una carpeta del disco de DashOS; DashOS no trae ninguno).
const PREFS_AMIGAOS: &[u8] = b"SYS:C/Prefs/amigaos";

/// Monta el AmigaOS del usuario, si lo dio en `SYS:C/Prefs/amigaos` (una línea: su carpeta, desde la raíz del disco):
/// `AmigaOS:` es esa carpeta (de solo leer: `SYS:` lo es) y sus `Libs`, `Classes`, `Devs`, `Fonts` y `Locale` van
/// detrás de las asignaciones de la caja (`LIBS:`, `Classes:`...), y `MUI:` si lo trae. Lo nativo gana siempre: ramlib
/// solo carga lo que no está. Devuelve lo que se dice por la consola.
pub(super) fn montar_amigaos<F: caja_dos::SistemaArchivos>(dos: &mut caja_dos::Dos<F>, cd: &mut CajaDos) -> alloc::string::String {
    let Ok(texto) = dos.leer_fichero(cd, PREFS_AMIGAOS, 4096) else {
        return alloc::string::String::from("AmigaOS del usuario: ninguno (SYS:C/Prefs/amigaos)");
    };
    let linea: Vec<u8> = texto.split(|&b| b == b'\n').next().unwrap_or(&[]).iter().copied().filter(|&b| b != b'\r').collect();
    let carpeta = core::str::from_utf8(&linea).unwrap_or("").trim().trim_start_matches('/');
    if carpeta.is_empty() || carpeta.contains(':') || carpeta.split('/').any(|p| p == ".." || p == ".") || carpeta.len() > 200 {
        return format!("AmigaOS del usuario: \"{}\" no vale (una carpeta del disco)", carpeta);
    }
    montar_amigaos_en(dos, cd, &format!("SYS:{}", carpeta), " (solo leer)")
}

/// `AmigaOS:` en `ruta` (un camino de dos), con sus cajones detrás de las asignaciones de la caja. Lo que se dice (con
/// `nota` tras la ruta).
pub(super) fn montar_amigaos_en<F: caja_dos::SistemaArchivos>(
    dos: &mut caja_dos::Dos<F>,
    cd: &mut CajaDos,
    ruta: &str,
    nota: &str,
) -> alloc::string::String {
    if let Err(e) = dos.asignar(cd, "AmigaOS", ruta) {
        return format!("AmigaOS del usuario: {} no se monta (error {})", ruta, e);
    }
    let mut hechas = Vec::new();
    for (nombre, sub) in [
        ("LIBS", "Libs"),
        ("Classes", "Classes"),
        ("DEVS", "Devs"),
        ("FONTS", "Fonts"),
        ("LOCALE", "Locale"),
        ("MUI", "MUI"),
        ("LIBS", "MUI/Libs"),
    ] {
        let destino = format!("AmigaOS:{}", sub);
        if dos.asignar_mas(cd, nombre, &destino).is_ok() {
            hechas.push(format!("{}: {}", nombre, destino));
        }
    }
    format!("AmigaOS del usuario en {}{}, detrás de lo nativo: {}", ruta, nota, hechas.join(", "))
}

/// Cuántas cargas se dicen por la consola (las demás solo se cuentan).
const DICHAS: u32 = 24;

/// Los sitios donde se busca `nombre` (una biblioteca o, con `dispositivo`, un dispositivo), en orden.
pub(crate) fn sitios(nombre: &[u8], dispositivo: bool) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    if nombre.contains(&b':') {
        v.push(nombre.to_vec());
        return v;
    }
    let con = |prefijo: &[u8]| {
        let mut r = prefijo.to_vec();
        r.extend_from_slice(nombre);
        r
    };
    if dispositivo {
        v.push(con(b"DEVS:"));
        v.push(con(b"PROGDIR:Devs/"));
    } else {
        v.push(con(b"LIBS:"));
        v.push(con(b"Classes:"));
        v.push(con(b"PROGDIR:Libs/"));
        v.push(con(b"PROGDIR:Classes/"));
    }
    v
}

/// El nombre sin el camino ("gadgets/layout.gadget" da "layout.gadget"), en minúsculas.
fn base(nombre: &[u8]) -> Vec<u8> {
    let k = nombre.iter().rposition(|&c| c == b'/' || c == b':').map_or(0, |k| k + 1);
    nombre[k..].iter().map(|c| c.to_ascii_lowercase()).collect()
}

/// Si un nombre no se carga nunca de disco.
pub(crate) fn rechazada(nombre: &[u8]) -> bool {
    let b = base(nombre);
    RECHAZADAS.contains(&b.as_slice())
}

/// La carga que pide exec: A1 = el nombre, D1 = 0 biblioteca, 1 dispositivo. Devuelve la lista de segmentos (un
/// BPTR) o 0.
pub(super) fn cargar(c: &mut Caja, nombre: u32, tipo: u32) -> u32 {
    let Some(n) = leer_nombre(c, nombre) else { return 0 };
    if n.is_empty() || n.iter().any(|&b| b < 32) {
        return 0;
    }
    let disp = tipo == caja_exec::ramlib::CARGAR_DISPOSITIVO;
    if SIN_SOPORTE.contains(&base(&n).as_slice()) {
        decir(c, &format!("ramlib: {} no se carga de disco (aún no: usa funciones privadas de exec)", latin1(&n)));
        apuntar_falta(c, &n, disp);
        return 0;
    }
    if rechazada(&n) {
        decir(c, &format!("ramlib: {} no se carga de disco (nunca: hablaría con el chipset)", latin1(&n)));
        apuntar_falta(c, &n, disp);
        return 0;
    }
    for ruta in sitios(&n, disp) {
        let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: true };
        if let Ok(seg) = c.dos.cargar_de_disco(&mut cd, &ruta) {
            let rangos: Vec<(u32, u32)> =
                caja_dos::hunk::segmentos(&cd, seg).iter().map(|&(d, n)| (d, d.saturating_add(n))).collect();
            // Código nuevo: el JIT olvida lo traducido (salvo lo que se está ejecutando, caja.c).
            // SEGURIDAD: una función de caja.c sin argumentos; no ejecuta 68k.
            unsafe { super::caja_jit_olvidar() };
            c.ramlib_cargas += 1;
            if c.ramlib_segs.len() < 256 {
                c.ramlib_segs.push((seg, rangos, latin1(&base(&n))));
            }
            decir(c, &format!("ramlib: {} cargada de disco", latin1(&ruta)));
            return seg;
        }
    }
    apuntar_falta(c, &n, disp);
    0
}

/// Una biblioteca o un dispositivo que no hay (ni nativo, ni de disco, o rechazado) va al registro de lo que falta.
fn apuntar_falta(c: &mut Caja, n: &[u8], disp: bool) {
    let q = super::faltan::quien(c, c.pc_llamada);
    let f = if disp { "OpenDevice" } else { "OpenLibrary" };
    if let Some(t) = c.faltan.apuntar(&latin1(&base(n)), f, &q) {
        di(&t);
    }
}

/// Suelta una lista de segmentos que cargó `cargar` (tras el `Expunge` de su biblioteca).
pub(super) fn soltar(c: &mut Caja, seglist: u32) {
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: true };
    caja_dos::hunk::descargar(&mut cd, seglist);
    c.ramlib_soltadas += 1;
    c.ramlib_segs.retain(|(s, _, _)| *s != seglist);
}

fn leer_nombre(c: &Caja, d: u32) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    for i in 0..256u32 {
        let b = c.ex.mem.leer8(d.checked_add(i)?).ok()?;
        if b == 0 {
            return Some(v);
        }
        v.push(b);
    }
    None
}

fn latin1(s: &[u8]) -> alloc::string::String {
    s.iter().map(|&b| b as char).collect()
}

fn decir(c: &mut Caja, t: &str) {
    if c.ramlib_dichas < DICHAS {
        c.ramlib_dichas += 1;
        di(t);
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn sitios_y_rechazadas() {
        assert_eq!(sitios(b"x.library", false)[0], b"LIBS:x.library".to_vec());
        assert_eq!(sitios(b"gadgets/layout.gadget", false)[1], b"Classes:gadgets/layout.gadget".to_vec());
        assert_eq!(sitios(b"SYS:Libs/x.library", false), vec![b"SYS:Libs/x.library".to_vec()]);
        assert_eq!(sitios(b"timer.device", true)[0], b"DEVS:timer.device".to_vec());
        for n in [&b"graphics.library"[..], b"LIBS:Graphics.Library", b"x/layers.library", b"DOS.library"] {
            assert!(rechazada(n), "{:?}", n);
        }
        assert!(!rechazada(b"muimaster.library"));
    }
}
