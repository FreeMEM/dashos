//! S27: los avisos en pantalla (OSD) como los de DMS, sin pintar: qué se enseña (volumen, brillo, Bloq Mayús, espacio
//! de trabajo), dónde, de qué tamaño, cuánto dura y cómo entra y sale. Lo pinta el programa `dock`.
//!
//! Portado de DankMaterialShell v2.1.2: `Widgets/DOSD.qml` (sitio por `osdPosition`, tiempos, escala 0,92 → 1 con el
//! muelle "default" y la opacidad con OutQuart), `Modules/OSD/LevelOSD.qml` y `OsdLevelRow.qml` (la fila del nivel: el
//! icono tonal, el deslizador "s" o "m" y el valor), `OsdIcon.qml`, `DCommon/Widgets/DSlider.qml` (la pista y el
//! tirador), `VolumeOSD.qml`, `BrightnessOSD.qml`, `CapsLockOSD.qml`, `WorkspaceOSD.qml` y `Common/OSDManager.qml` (uno
//! por monitor). Copyright (c) 2025 Avenge Media LLC, licencia MIT (`docs/licencias/DankMaterialShell-MIT.txt`).
//!
//! Sin nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/osd_modelo.rs`.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[cfg(test)]
#[path = "dock_ajustes.rs"]
#[allow(dead_code)]
pub mod ajustes;
#[cfg(test)]
#[path = "muelle.rs"]
#[allow(dead_code)]
pub mod muelle;
#[cfg(not(test))]
use crate::{ajustes, muelle};

use ajustes::Osd as AjustesOsd;
use muelle::{Curva, Muelle, Preset, Tramo, UNO};

/// `Theme.osdHeight`, `osdLevelWidth`, `osdLevelVerticalHeight`, `buttonHeightS`, `buttonHeightM`, `spacingS/M/L`.
pub const ALTO_OSD: i32 = 60;
pub const ANCHO_NIVEL: i32 = 264;
pub const ALTO_NIVEL_VERTICAL: i32 = 352;
pub const BOTON_S: i32 = 40;
pub const BOTON_M: i32 = 56;
pub const ESPACIO_S: i32 = 8;
pub const ESPACIO_M: i32 = 12;
pub const ESPACIO_L: i32 = 16;
/// Lo que se deja alrededor para la sombra (nivel 2 de elevación).
pub const SOMBRA: i32 = 8;
/// `Theme.popupEnterScale`.
pub const ESCALA_ENTRADA: i64 = UNO * 92 / 100;

/// Lo que se enseña.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Que {
    Volumen { nivel: u32, silencio: bool },
    /// Cuando haya quien lleve el brillo (hoy no hay servicio: `BrightnessOSD` queda listo).
    Brillo { nivel: u32 },
    Mayusculas { activo: bool },
    /// "Espacio de trabajo N".
    Espacio { numero: u32 },
}

impl Que {
    /// `osdKind`.
    pub fn tipo(&self) -> &'static str {
        match self {
            Que::Volumen { .. } => "volumen",
            Que::Brillo { .. } => "brillo",
            Que::Mayusculas { .. } => "mayusculas",
            Que::Espacio { .. } => "espacio",
        }
    }
    /// `autoHideInterval`: el nivel 3 s, Bloq Mayús 2 s, el espacio 1,5 s.
    pub fn dura_us(&self) -> u64 {
        match self {
            Que::Volumen { .. } | Que::Brillo { .. } => 3_000_000,
            Que::Mayusculas { .. } => 2_000_000,
            Que::Espacio { .. } => 1_500_000,
        }
    }
    /// Si está puesto en los ajustes.
    pub fn permitido(&self, a: &AjustesOsd) -> bool {
        match self {
            Que::Volumen { .. } => a.volumen,
            Que::Brillo { .. } => a.brillo,
            Que::Mayusculas { .. } => a.mayusculas,
            Que::Espacio { .. } => a.espacio,
        }
    }
    /// El texto del espacio (`Workspace %1`).
    pub fn etiqueta(&self) -> String {
        match self {
            Que::Espacio { numero } => alloc::format!("Espacio de trabajo {}", numero),
            Que::Volumen { silencio: true, .. } => "Silencio".to_string(),
            Que::Volumen { nivel, .. } | Que::Brillo { nivel } => alloc::format!("{}%", nivel),
            Que::Mayusculas { activo } => (if *activo { "Bloq Mayús" } else { "Sin Bloq Mayús" }).to_string(),
        }
    }
}

/// Los dibujos (los de Material Symbols que usa DMS; hasta que estén, el dock los traza).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glifo {
    /// `volume_up`, `volume_down`, `volume_mute`, `volume_off`.
    Volumen(u32),
    Silencio,
    /// `brightness_*`.
    Brillo,
    /// `shift_lock` y `shift_lock_off`.
    BloqMayus(bool),
    /// `view_module`.
    Espacios,
    /// El de la salida (`speaker`) o la pantalla (`monitor`), al final cuando no se enseña el valor.
    Altavoz,
    Pantalla,
}

/// Los papeles del tema (el dock los pasa a colores del kit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Papel {
    /// `Theme.hostSurface` (la superficie).
    Superficie,
    Primario,
    SobrePrimario,
    /// `Theme.secondaryContainer`, la pista vacía.
    ContenedorSecundario,
    SobreSuperficie,
}

/// Una pieza del OSD, en coordenadas del OSD (sin la sombra).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pieza {
    /// Un rectángulo redondeado (radios: arriba izq., arriba der., abajo izq., abajo der.).
    Caja { x: i32, y: i32, w: i32, h: i32, radios: [i32; 4], papel: Papel },
    Glifo { x: i32, y: i32, lado: i32, glifo: Glifo, papel: Papel },
    /// Un texto centrado en la caja, de `tam` (12: `fontSizeSmall`; 14: `fontSizeMedium`), medium.
    Texto { x: i32, y: i32, w: i32, h: i32, texto: String, tam: u32, papel: Papel, centrado: bool },
}

/// `Shape.fullRadius` con la redondez de los ajustes: la mitad del lado corto, por la escala (como mucho 1).
pub fn redondo(w: i32, h: i32, redondez: u32) -> i32 {
    let mitad = w.min(h).max(0) / 2;
    mitad * redondez.min(50) as i32 / 50
}

/// `Shape.scaledRadius`: un radio por la escala de la redondez, sin pasar de `limite`.
fn escalado(radio: i32, limite: i32, redondez: u32) -> i32 {
    (radio * redondez as i32 / 50).clamp(0, limite)
}

/// Lo que mide el OSD de `que` (sin la sombra), en una pantalla de `pantalla`. `ancho_texto`: lo que mide la etiqueta del
/// espacio (14, medium) y una cifra.
pub fn medir(que: &Que, a: &AjustesOsd, pantalla: (i32, i32), ancho_texto: (i32, i32)) -> (i32, i32) {
    let (sw, sh) = pantalla;
    match que {
        Que::Volumen { .. } | Que::Brillo { .. } => {
            if a.posicion.vertical() {
                (ALTO_OSD, ALTO_NIVEL_VERTICAL.min(sh - ESPACIO_M * 2).max(ALTO_OSD))
            } else {
                // `osdValueReserve`: con el icono del final (siempre en estos dos), 56.
                ((ANCHO_NIVEL + BOTON_M).min(sw - ESPACIO_M * 2).max(ALTO_OSD), BOTON_S + ESPACIO_S * 2)
            }
        }
        Que::Mayusculas { .. } => (ALTO_OSD, ALTO_OSD),
        Que::Espacio { .. } => ((ESPACIO_L * 2 + BOTON_S + ESPACIO_M + ancho_texto.0 + ancho_texto.1).max(140).min(sw - ESPACIO_M * 2).max(ALTO_OSD), ALTO_OSD),
    }
}

/// Dónde va en la pantalla (la esquina del OSD sin sombra), con `area` lo que dejan las barras y el dock
/// (`edgeInset`: 12 más lo que ocupe la barra de ese lado).
pub fn sitio(a: &AjustesOsd, tam: (i32, i32), area: (i32, i32, i32, i32)) -> (i32, i32) {
    let (ax, ay) = a.posicion.alinear();
    let (x0, y0, w, h) = area;
    let uno = |al: i32, ini: i32, largo: i32, t: i32| match al {
        -1 => ini + ESPACIO_M,
        1 => ini + largo - t - ESPACIO_M,
        _ => ini + (largo - t) / 2,
    };
    (uno(ax, x0, w, tam.0), uno(ay, y0, h, tam.1))
}

/// Las piezas del OSD de `que`, de `tam`.
pub fn piezas(que: &Que, a: &AjustesOsd, tam: (i32, i32)) -> Vec<Pieza> {
    let (w, h) = tam;
    let rz = a.redondez;
    let r = redondo(w, h, rz);
    let mut p = alloc::vec![Pieza::Caja { x: 0, y: 0, w, h, radios: [r; 4], papel: Papel::Superficie }];
    match que {
        Que::Volumen { .. } | Que::Brillo { .. } => p.extend(fila_nivel(que, a, tam)),
        Que::Mayusculas { activo } => {
            // `OsdIcon` sin tono, en el primario, centrado.
            let lado = 24;
            p.push(Pieza::Glifo { x: (w - lado) / 2, y: (h - lado) / 2, lado, glifo: Glifo::BloqMayus(*activo), papel: Papel::Primario });
        }
        Que::Espacio { .. } => {
            // Una fila con relleno de 16: el icono tonal (40) y el texto de 14 medium.
            let ic = BOTON_S;
            let ry = (h - ic) / 2;
            let rr = redondo(ic, ic, rz);
            p.push(Pieza::Caja { x: ESPACIO_L, y: ry, w: ic, h: ic, radios: [rr; 4], papel: Papel::ContenedorSecundario });
            p.push(Pieza::Glifo { x: ESPACIO_L + (ic - 24) / 2, y: ry + (ic - 24) / 2, lado: 24, glifo: Glifo::Espacios, papel: Papel::SobreSuperficie });
            let tx = ESPACIO_L + ic + ESPACIO_M;
            p.push(Pieza::Texto { x: tx, y: 0, w: (w - tx - ESPACIO_L).max(0), h, texto: que.etiqueta(), tam: 14, papel: Papel::SobreSuperficie, centrado: false });
        }
    }
    p
}

fn glifo_nivel(que: &Que) -> Glifo {
    match que {
        Que::Volumen { silencio: true, .. } | Que::Volumen { nivel: 0, .. } => Glifo::Silencio,
        // `sinkVolumeIconName`: hasta 33 "mute", hasta 66 "down", más "up".
        Que::Volumen { nivel, .. } => Glifo::Volumen(if *nivel <= 33 { 1 } else if *nivel <= 66 { 2 } else { 3 }),
        _ => Glifo::Brillo,
    }
}

/// `OsdLevelRow`: el icono tonal (primario con su glifo en sobre-primario), el deslizador y, al final, el valor (con
/// `siempre_valor`) o el icono de la salida.
fn fila_nivel(que: &Que, a: &AjustesOsd, (w, h): (i32, i32)) -> Vec<Pieza> {
    let rz = a.redondez;
    let vertical = a.posicion.vertical();
    let (nivel, texto) = match que {
        Que::Volumen { nivel, silencio } => (if *silencio { 0 } else { *nivel }, que.etiqueta()),
        Que::Brillo { nivel } => (*nivel, que.etiqueta()),
        _ => (0, String::new()),
    };
    let nivel = nivel.min(100) as i32;
    let relleno = ESPACIO_S;
    let traves = if vertical { w } else { h };
    // El deslizador: "s" en horizontal (pista 24, tirador 36), "m" en vertical (40 y 52).
    let (pista, tirador, esquina) = if vertical { (40, 52, 12) } else { (24, 36, 8) };
    let icono = if vertical { pista.min(traves - relleno * 2) } else { BOTON_S.min(traves - relleno * 2) };
    let extremo = relleno.max((traves - icono) / 2);
    let entre = if vertical { ESPACIO_M } else { ESPACIO_S };
    let valor = icono + entre;
    let mut p = Vec::new();
    let ri = redondo(icono, icono, rz);
    let glifo = 24.min(icono);
    // El icono, al principio del eje.
    let (ix, iy) = if vertical { ((w - icono) / 2, extremo) } else { (extremo, (h - icono) / 2) };
    p.push(Pieza::Caja { x: ix, y: iy, w: icono, h: icono, radios: [ri; 4], papel: Papel::Primario });
    p.push(Pieza::Glifo { x: ix + (icono - glifo) / 2, y: iy + (icono - glifo) / 2, lado: glifo, glifo: glifo_nivel(que), papel: Papel::SobrePrimario });
    // El hueco del deslizador.
    let (sx, sy, sw, sh) = if vertical {
        let y = extremo + icono + entre;
        (relleno, y, w - relleno * 2, (h - y - extremo - valor).max(0))
    } else {
        let x = extremo + icono + entre;
        (x, 0, (w - extremo * 2 - icono - entre - valor).max(0), h)
    };
    let largo = if vertical { sh } else { sw };
    // `DSlider`: el tirador de 4 recorre `largo - 4`; la pista llena hasta 6 antes, la vacía desde 6 después.
    let tw = 4;
    let recorrido = (largo - tw).max(0);
    let pos = tw / 2 + recorrido * nivel / 100 - tw / 2;
    let hueco = 6;
    let fuera = escalado(esquina, pista / 2, rz);
    let dentro = (2 * rz as i32 / 50).min(pista / 2);
    let lleno = (pos - hueco).max(0);
    let vacio_ini = pos + tw + hueco;
    let vacio = (largo - vacio_ini).max(0);
    if vertical {
        // De abajo arriba: lo lleno abajo.
        let cx = sx + (sw - pista) / 2;
        let base = sy + sh;
        if lleno > 0 {
            p.push(Pieza::Caja { x: cx, y: base - lleno, w: pista, h: lleno, radios: [dentro, dentro, fuera, fuera], papel: Papel::Primario });
        }
        if vacio > 0 {
            p.push(Pieza::Caja { x: cx, y: sy, w: pista, h: vacio, radios: [fuera, fuera, dentro, dentro], papel: Papel::ContenedorSecundario });
        }
        p.push(Pieza::Caja { x: sx + (sw - tirador) / 2, y: base - pos - tw, w: tirador, h: tw, radios: [tw / 2; 4], papel: Papel::Primario });
    } else {
        let cy = sy + (sh - pista) / 2;
        if lleno > 0 {
            p.push(Pieza::Caja { x: sx, y: cy, w: lleno, h: pista, radios: [fuera, dentro, fuera, dentro], papel: Papel::Primario });
        }
        if vacio > 0 {
            p.push(Pieza::Caja { x: sx + vacio_ini, y: cy, w: vacio, h: pista, radios: [dentro, fuera, dentro, fuera], papel: Papel::ContenedorSecundario });
        }
        p.push(Pieza::Caja { x: sx + pos, y: sy + (sh - tirador) / 2, w: tw, h: tirador, radios: [tw / 2; 4], papel: Papel::Primario });
    }
    // Al final: el valor (12, medium, en el primario) o el icono de la salida.
    let (vx, vy) = if vertical { ((w - icono) / 2, h - extremo - icono) } else { (w - extremo - icono, (h - icono) / 2) };
    if a.siempre_valor {
        p.push(Pieza::Texto { x: vx - 4, y: vy, w: icono + 8, h: icono, texto, tam: 12, papel: Papel::Primario, centrado: true });
    } else {
        let g = if matches!(que, Que::Brillo { .. }) { Glifo::Pantalla } else { Glifo::Altavoz };
        p.push(Pieza::Glifo { x: vx + (icono - 24) / 2, y: vy + (icono - 24) / 2, lado: 24, glifo: g, papel: Papel::Primario });
    }
    p
}

/// El estado del OSD de un monitor (`DOSD` + `OSDManager`: uno a la vez; el nuevo sustituye al que se veía).
#[derive(Clone, Debug)]
pub struct Estado {
    pub que: Option<Que>,
    /// `shouldBeVisible`: se enseña (o va entrando); falso mientras sale.
    pub visible: bool,
    /// Hasta cuándo se ve (`hideTimer`) y cuándo deja de existir tras salir (`closeTimer`).
    pub hasta_us: u64,
    pub fin_us: Option<u64>,
    /// La escala (muelle "default" de la duración media) y la opacidad (OutQuart, duración media).
    pub escala: Muelle,
    pub opacidad: Tramo,
    /// El ratón encima: no se va (`enableMouseInteraction` de los de nivel).
    pub raton: bool,
    /// Cuántas veces se pidió desde que salió (una ráfaga de teclas es un solo OSD).
    pub juntadas: u32,
}

impl Default for Estado {
    fn default() -> Self {
        let mut escala = Muelle::preset(Preset::DeSerie, muelle::media_ms(muelle::BASE_MS));
        escala.saltar(ESCALA_ENTRADA);
        Estado { que: None, visible: false, hasta_us: 0, fin_us: None, escala, opacidad: Tramo::quieto(0), raton: false, juntadas: 0 }
    }
}

/// Lo que cambia al enseñar: si hay que crear la capa (no existía), solo repintarla, o nada (no está permitido).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cambio {
    Nada,
    Nuevo,
    Repintar,
}

impl Estado {
    /// `show()`: si ya se ve, se alarga; si no, entra. Lo que se enseña pasa a ser `que`.
    pub fn ensenar(&mut self, que: Que, a: &AjustesOsd, ahora_us: u64) -> Cambio {
        if !que.permitido(a) {
            return Cambio::Nada;
        }
        let dur = que.dura_us();
        let media = muelle::media_ms(muelle::BASE_MS) as u64 * 1000;
        let habia = self.que.is_some() && self.fin_us.is_none_or(|f| ahora_us < f);
        self.que = Some(que);
        self.hasta_us = ahora_us.saturating_add(dur);
        self.fin_us = None;
        self.juntadas = self.juntadas.saturating_add(1);
        if !self.visible {
            self.visible = true;
            self.escala.ir(UNO);
            self.opacidad = self.opacidad.hacia(ahora_us, UNO, media, Curva::SalidaCuartica);
        }
        if habia { Cambio::Repintar } else { Cambio::Nuevo }
    }

    /// Cambia lo que se enseña sin alargarlo (el volumen que llega después de la tecla), si se ve.
    pub fn poner(&mut self, que: Que) -> bool {
        if self.visible && self.que.as_ref().is_some_and(|q| q.tipo() == que.tipo()) && self.que.as_ref() != Some(&que) {
            self.que = Some(que);
            return true;
        }
        false
    }

    /// Avanza el tiempo: vence el plazo (sale), avanzan la escala y la opacidad y, acabada la salida, desaparece.
    /// Devuelve `(sigue_animando, cerrar)`: si hay que repintar y si hay que quitar la capa.
    pub fn avanzar(&mut self, ahora_us: u64, us: u64) -> (bool, bool) {
        if self.que.is_none() {
            return (false, false);
        }
        let media = muelle::media_ms(muelle::BASE_MS) as u64 * 1000;
        if self.visible && ahora_us >= self.hasta_us {
            if self.raton {
                self.hasta_us = ahora_us.saturating_add(self.que.as_ref().map_or(0, Que::dura_us));
            } else {
                // `hide()`: sale, y deja de existir al acabar (`closeTimer`: la animación más 50 ms).
                self.visible = false;
                self.escala.ir(ESCALA_ENTRADA);
                self.opacidad = self.opacidad.hacia(ahora_us, 0, media, Curva::SalidaCuartica);
                self.fin_us = Some(ahora_us.saturating_add(media + 50_000));
            }
        }
        let anima = self.escala.avanzar(us) | !self.opacidad.acabado(ahora_us);
        if self.fin_us.is_some_and(|f| ahora_us >= f) {
            self.que = None;
            self.fin_us = None;
            self.juntadas = 0;
            return (false, true);
        }
        (anima, false)
    }

    /// Cuánto falta para lo siguiente que pasa (el plazo o el cierre).
    pub fn proximo_us(&self, ahora_us: u64) -> Option<u64> {
        self.que.as_ref()?;
        let t = if self.visible { self.hasta_us } else { self.fin_us.unwrap_or(ahora_us) };
        Some(t.saturating_sub(ahora_us))
    }

    /// La escala y la opacidad de ahora, en 1/65536.
    pub fn transformacion(&self, ahora_us: u64) -> (i64, i64) {
        (self.escala.valor, self.opacidad.valor(ahora_us))
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use ajustes::PosOsd;

    #[test]
    fn el_volumen_de_shiva() {
        let a = AjustesOsd::de_serie();
        let q = Que::Volumen { nivel: 40, silencio: false };
        let tam = medir(&q, &a, (1920, 1080), (0, 0));
        assert_eq!(tam, (320, 56));
        // Arriba en el centro, a 12 de lo que deja la barra (la barra de DashOS: 40).
        assert_eq!(sitio(&a, tam, (0, 40, 1920, 1040)), (800, 52));
        let p = piezas(&q, &a, tam);
        // El fondo: la píldora con la redondez de shiva (38 de 50: 21 de 28).
        assert_eq!(p[0], Pieza::Caja { x: 0, y: 0, w: 320, h: 56, radios: [21; 4], papel: Papel::Superficie });
        // El icono de 40 a 8 del borde, y el deslizador de 56 a 264 (208).
        assert!(matches!(p[1], Pieza::Caja { x: 8, y: 8, w: 40, h: 40, papel: Papel::Primario, .. }));
        let llena = p.iter().find(|x| matches!(x, Pieza::Caja { y: 16, h: 24, papel: Papel::Primario, .. })).expect("lo lleno");
        if let Pieza::Caja { x, w, .. } = llena {
            assert_eq!(*x, 56);
            assert_eq!(*w, 4 / 2 + 204 * 40 / 100 - 2 - 6);
        }
        assert!(p.iter().any(|x| matches!(x, Pieza::Texto { texto, papel: Papel::Primario, .. } if texto == "40%")));
    }

    #[test]
    fn todas_las_posiciones_dentro() {
        let mut a = AjustesOsd::de_serie();
        for (pos, _) in ajustes::POSICIONES_OSD {
            a.posicion = pos;
            for q in [Que::Volumen { nivel: 100, silencio: true }, Que::Brillo { nivel: 0 }, Que::Mayusculas { activo: true }, Que::Espacio { numero: 9 }] {
                for pantalla in [(1920, 1080), (800, 600), (200, 150)] {
                    let tam = medir(&q, &a, pantalla, (120, 8));
                    let (x, y) = sitio(&a, tam, (0, 0, pantalla.0, pantalla.1));
                    assert!(x >= 0 && y >= 0 && x + tam.0 <= pantalla.0.max(tam.0) && y + tam.1 <= pantalla.1.max(tam.1), "{pos:?} {q:?} {pantalla:?}");
                    for pz in piezas(&q, &a, tam) {
                        if let Pieza::Caja { x, y, w, h, .. } = pz {
                            assert!(x >= 0 && y >= 0 && x + w <= tam.0 && y + h <= tam.1, "{pos:?} {q:?}: pieza fuera");
                        }
                    }
                }
            }
        }
        a.posicion = PosOsd::IzquierdaCentro;
        assert_eq!(medir(&Que::Volumen { nivel: 1, silencio: false }, &a, (1920, 1080), (0, 0)), (60, 352));
    }

    #[test]
    fn rafaga_y_tiempos() {
        let a = AjustesOsd::de_serie();
        let mut e = Estado::default();
        assert_eq!(e.ensenar(Que::Volumen { nivel: 50, silencio: false }, &a, 0), Cambio::Nuevo);
        // Mil teclas seguidas: el mismo OSD, alargado.
        for k in 1..1000u64 {
            assert_eq!(e.ensenar(Que::Volumen { nivel: (50 + k % 50) as u32, silencio: false }, &a, k * 1000), Cambio::Repintar);
        }
        assert_eq!(e.juntadas, 1000);
        let mut t = 999_000;
        let mut cerrado = false;
        while t < 10_000_000 {
            t += 16_667;
            let (_, c) = e.avanzar(t, 16_667);
            if c {
                cerrado = true;
                break;
            }
        }
        assert!(cerrado);
        // 3 s tras la última tecla, más 150 ms de salida y 50 de margen.
        assert!(t >= 999_000 + 3_000_000 + 200_000 && t < 999_000 + 3_000_000 + 250_000, "{t}");
        assert!(e.que.is_none());
        // El del espacio no está puesto de serie (como DMS).
        assert_eq!(e.ensenar(Que::Espacio { numero: 2 }, &a, t), Cambio::Nada);
        assert_eq!(e.ensenar(Que::Mayusculas { activo: true }, &a, t), Cambio::Nuevo);
        // Otro tipo mientras se ve: lo sustituye.
        assert_eq!(e.ensenar(Que::Volumen { nivel: 3, silencio: false }, &a, t + 1000), Cambio::Repintar);
        assert_eq!(e.que, Some(Que::Volumen { nivel: 3, silencio: false }));
    }

    #[test]
    fn entra_con_escala_y_opacidad() {
        let a = AjustesOsd::de_serie();
        let mut e = Estado::default();
        e.ensenar(Que::Mayusculas { activo: false }, &a, 0);
        let (s0, o0) = e.transformacion(0);
        assert_eq!((s0, o0), (ESCALA_ENTRADA, 0));
        let mut t = 0;
        while e.avanzar(t, 16_667).0 {
            t += 16_667;
        }
        assert_eq!(e.transformacion(t), (UNO, UNO));
        assert!(t <= 400_000, "{t}");
    }

    #[test]
    fn hostil_niveles_y_relojes() {
        let a = AjustesOsd::de_serie();
        let mut e = Estado::default();
        e.ensenar(Que::Volumen { nivel: u32::MAX, silencio: false }, &a, u64::MAX - 10);
        let p = piezas(e.que.as_ref().unwrap(), &a, (320, 56));
        assert!(!p.is_empty());
        e.avanzar(0, u64::MAX);
        e.avanzar(u64::MAX, 0);
        // Con el reloj al final ya se cerró: no debe romper nada.
        let _ = e.proximo_us(0);
        // Sin nada, no hace nada.
        let mut f = Estado::default();
        assert_eq!(f.avanzar(5, 5), (false, false));
        assert!(f.proximo_us(5).is_none());
        assert!(!f.poner(Que::Brillo { nivel: 5 }));
    }
}
