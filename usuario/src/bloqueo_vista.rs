//! S27, oleada 6: dónde va cada cosa en la pantalla de bloqueo (y en la de entrada) de `guardian`, como la de DMS, y sus
//! textos. Sin dibujo: lo pinta `guardian`.
//!
//! Portado de DMS (DankMaterialShell v2.1.2: `Modules/Plugins/WidgetEditLayer.qml` (`stockRect`, dónde va cada pieza),
//! `DCommon/Session/LockMetrics.qml`, `DCommon/Widgets/DClockWidget.qml` y `DClockFace.qml` (el reloj "expressive"),
//! `Modules/Lock/Widgets/LockAuthWidget.qml` (el avatar y el campo), `LockNotificationsWidget.qml` y
//! `DCommon/Common/settings/SharedSettingsSpec.js` (`lockWidgetDefaults`); MIT, copyright (c) 2025 Avenge Media LLC;
//! aviso entero en `docs/licencias/DankMaterialShell-MIT.txt`). Lo de serie de DMS: el reloj arriba en el centro (a
//! `edgeInset`, 48), de 240 × 120 por 1,75; el avatar y la contraseña en medio (56 de alto, el campo de 312); los avisos
//! debajo; el estado arriba a la derecha (a 24). Lo que DMS no tiene y aquí hace falta: el nombre de quien entra y su
//! frase (S6b: así se sabe que es el sistema) encima del campo, y las teclas al pie.
//!
//! En una pantalla baja (800 × 600) el reloj se encoge para que quepa lo de en medio.
//!
//! Sin nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/bloqueo_vista.rs`.

#[cfg(test)]
extern crate alloc;

use alloc::format;
use alloc::string::String;

/// Un rectángulo (x, y, ancho, alto).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Caja {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Caja {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Caja {
        Caja { x, y, w, h }
    }
    pub fn abajo(&self) -> i32 {
        self.y + self.h
    }
}

/// Medidas de DMS (`Theme`, `LockMetrics`).
pub const BORDE: i32 = 48; // edgeInset: spacingXL × 2
pub const MARGEN_ESTADO: i32 = 24; // spacingXL
pub const ALTO_CAMPO: i32 = 56; // buttonHeightM
pub const ANCHO_CAMPO: i32 = 200 + 56 * 2; // fieldDefaultWidth + buttonHeightM × 2
pub const SEPARACION: i32 = 12; // spacingM
pub const ANCHO_FILA: i32 = ANCHO_CAMPO + ALTO_CAMPO + SEPARACION; // passwordRowWidth
/// El reloj "expressive" en el bloqueo: 240 × 120 por 1,75.
pub const RELOJ: (i32, i32) = (420, 210);
/// Los avisos en chips (`buttonHeightXS`) y su ancho (el de la fila).
pub const ALTO_CHIP: i32 = 32;
/// Los iconos de la fila de estado (`iconSizeSmall` en DMS; un poco más, para que se lean a 1920).
pub const ICONO_ESTADO: i32 = 20;

/// Dónde va cada cosa en una pantalla de `ancho` × `alto`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disposicion {
    /// La caja del reloj (los dígitos van centrados dentro) y la de la fecha, debajo.
    pub reloj: Caja,
    pub fecha: Caja,
    /// Lo de en medio: el nombre y la frase, y la fila del avatar y el campo.
    pub nombre: Caja,
    pub frase: Caja,
    pub avatar: Caja,
    pub campo: Caja,
    /// Lo que se dice bajo el campo (el error, Bloq Mayús), los avisos y el pie (las teclas).
    pub aviso: Caja,
    pub avisos: Caja,
    pub pie_y: i32,
    /// La fila de estado: su esquina de arriba a la derecha.
    pub estado: (i32, i32),
    /// Por dónde empiezan los formularios y la lista (la pantalla de entrada): bajo la fecha.
    pub arriba_formularios: i32,
}

/// La fila del avatar y el campo, y lo demás alrededor, en una pantalla de `ancho` × `alto`.
pub fn disponer(ancho: i32, alto: i32) -> Disposicion {
    let ancho = ancho.max(1);
    let alto = alto.max(1);
    // El reloj: el de DMS (420 × 210 a 48 del borde) si cabe; si no, más pequeño (como poco 80 de alto), para que entre
    // el reloj, la fecha y lo de en medio.
    let alto_reloj = RELOJ.1.min(alto / 5).max(80).min(alto / 3);
    let ancho_reloj = (alto_reloj * 2).min(ancho - 32).max(1);
    let borde = BORDE.min(alto / 24);
    let reloj = Caja::new((ancho - ancho_reloj) / 2, borde, ancho_reloj, alto_reloj);
    let fecha = Caja::new(reloj.x - 100, reloj.abajo() + 4, ancho_reloj + 200, 28);
    // La fila, en el centro de la pantalla (como `lockAuth`); si el reloj llega, debajo de él.
    let fila_ancho = ANCHO_FILA.min(ancho - 32);
    let medio_y = (alto / 2 - ALTO_CAMPO / 2).max(fecha.abajo() + 72);
    let fila_x = (ancho - fila_ancho) / 2;
    let avatar = Caja::new(fila_x, medio_y, ALTO_CAMPO, ALTO_CAMPO);
    let campo = Caja::new(fila_x + ALTO_CAMPO + SEPARACION, medio_y, fila_ancho - ALTO_CAMPO - SEPARACION, ALTO_CAMPO);
    let nombre = Caja::new(fila_x - 80, medio_y - 64, fila_ancho + 160, 26);
    let frase = Caja::new(fila_x - 120, medio_y - 34, fila_ancho + 240, 24);
    let aviso = Caja::new(fila_x - 80, campo.abajo() + 8, fila_ancho + 160, 22);
    let avisos = Caja::new(fila_x, aviso.abajo() + 16, fila_ancho, ALTO_CHIP);
    let pie_y = (alto - 24 - 36).max(avisos.abajo() + 16);
    Disposicion {
        reloj,
        fecha,
        nombre,
        frase,
        avatar,
        campo,
        aviso,
        avisos,
        pie_y,
        estado: (ancho - MARGEN_ESTADO, MARGEN_ESTADO),
        arriba_formularios: fecha.abajo() + 24,
    }
}

impl Disposicion {
    /// Lo que cambia al teclear en la contraseña: del nombre a los avisos (se repinta solo eso).
    pub fn franja_clave(&self, ancho: i32) -> Caja {
        let y = self.nombre.y - 4;
        Caja::new(0, y, ancho, self.avisos.abajo() + 4 - y)
    }
}

/// El tamaño de las cifras del reloj en una caja de `alto` (las de DMS ocupan la caja: en Roboto, la tinta de una cifra
/// es unos 0,71 del tamaño, y el ancho de "00:00", unos 2,7; se elige lo que cabe a lo alto y a lo ancho).
pub fn letra_reloj(caja: &Caja) -> u32 {
    let por_alto = caja.h * 100 / 100;
    let por_ancho = caja.w * 10 / 27;
    por_alto.min(por_ancho).max(8) as u32
}

/// Las horas y los minutos, con dos cifras (DMS: 24 horas de serie en castellano).
pub fn hora(h: u32, m: u32) -> (String, String) {
    (format!("{:02}", h % 24), format!("{:02}", m % 60))
}

pub const DIAS_LARGOS: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
pub const MESES_LARGOS: [&str; 12] =
    ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];

/// La fecha larga ("martes, 17 de noviembre"), como la de DMS (`dddd, MMMM d`, la del usuario en shiva) en castellano.
/// `semana` de 0 (lunes) a 6, `mes` de 1 a 12.
pub fn fecha_larga(semana: u32, dia: u32, mes: u32) -> String {
    let d = DIAS_LARGOS.get(semana as usize).copied().unwrap_or("");
    let m = MESES_LARGOS.get((mes as usize).wrapping_sub(1)).copied().unwrap_or("");
    let mut t = format!("{}, {} de {}", d, dia, m);
    // La primera en mayúscula, como al empezar una frase.
    if let Some(c) = t.chars().next() {
        let resto = String::from(&t[c.len_utf8()..]);
        t = c.to_uppercase().collect::<String>() + &resto;
    }
    t
}

/// El texto del contador ("3 avisos nuevos"), o nada si no hay.
pub fn texto_contador(n: u32) -> Option<String> {
    match n {
        0 => None,
        1 => Some(String::from("1 aviso nuevo")),
        n => Some(format!("{} avisos nuevos", n)),
    }
}

/// El texto de un chip de un programa ("correo" y, si hay más de uno, su número aparte).
pub fn texto_chip(nombre: &str, n: u32) -> (String, Option<String>) {
    let limpio: String = nombre.chars().filter(|c| !c.is_control()).take(16).collect();
    (if limpio.is_empty() { String::from("?") } else { limpio }, (n > 1).then(|| format!("{}", n.min(999))))
}

/// La inicial del avatar (DMS pone la foto o el icono de la persona; aquí, la inicial en mayúscula).
pub fn inicial(nombre: &str) -> String {
    nombre.chars().find(|c| c.is_alphanumeric()).map_or(String::from("?"), |c| c.to_uppercase().collect())
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn dentro(a: &Caja, ancho: i32, alto: i32) -> bool {
        a.x >= -200 && a.y >= 0 && a.x + a.w <= ancho + 200 && a.y + a.h <= alto
    }

    #[test]
    fn en_full_hd_como_dms() {
        let d = disponer(1920, 1080);
        assert_eq!(d.reloj, Caja::new(750, 45, 420, 210));
        assert_eq!(d.avatar.h, 56);
        assert_eq!(d.campo.w, 312);
        assert_eq!(d.avatar.x + ANCHO_FILA, d.campo.x + d.campo.w);
        // La fila en el centro, como `lockAuth`.
        assert_eq!(d.avatar.y, 540 - 28);
        assert!(d.avisos.y > d.campo.abajo());
        assert_eq!(d.estado, (1896, 24));
        assert!(d.pie_y > d.avisos.abajo());
    }

    #[test]
    fn nada_se_pisa_en_pantallas_bajas() {
        for (w, h) in [(800, 600), (640, 480), (1024, 768), (1920, 1080), (3840, 2160)] {
            let d = disponer(w, h);
            assert!(d.fecha.abajo() < d.nombre.y, "{}x{}: el reloj pisa el nombre", w, h);
            assert!(d.campo.abajo() < d.aviso.y + 1);
            for c in [d.reloj, d.avatar, d.campo] {
                assert!(dentro(&c, w, h), "{}x{}: {:?} se sale", w, h, c);
            }
        }
        // Medidas absurdas: no hay pánico.
        for (w, h) in [(0, 0), (-5, 10), (1, 1), (320, 240), (i32::MAX / 4, 3)] {
            let _ = disponer(w, h);
        }
    }

    #[test]
    fn textos() {
        assert_eq!(hora(7, 5), (String::from("07"), String::from("05")));
        assert_eq!(fecha_larga(1, 17, 11), "Martes, 17 de noviembre");
        assert_eq!(fecha_larga(2, 1, 1), "Miércoles, 1 de enero");
        // Fuera de rango: no hay pánico.
        assert_eq!(fecha_larga(9, 3, 0), ", 3 de ");
        assert_eq!(texto_contador(0), None);
        assert_eq!(texto_contador(1).as_deref(), Some("1 aviso nuevo"));
        assert_eq!(texto_contador(12).as_deref(), Some("12 avisos nuevos"));
        assert_eq!(texto_chip("correo", 1), (String::from("correo"), None));
        assert_eq!(texto_chip("a\nb\x07c", 5000).1.as_deref(), Some("999"));
        assert_eq!(texto_chip("\n", 2).0, "?");
        assert_eq!(inicial("ana"), "A");
        assert_eq!(inicial("élia"), "É");
        assert_eq!(inicial(""), "?");
    }

    #[test]
    fn la_letra_del_reloj_cabe() {
        let d = disponer(1920, 1080);
        let l = letra_reloj(&d.reloj);
        assert!(l >= 140 && l <= 210, "{}", l);
        let d = disponer(800, 600);
        assert!(letra_reloj(&d.reloj) * 27 / 10 <= d.reloj.w as u32 + 2);
    }
}
