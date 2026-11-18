//! Los discos del escritorio (rama `dock-discos`): qué tarjetas hay, dónde va cada una y cómo se mueven. Lo pinta el
//! Workbench (`bin/workbench.rs`); aquí no hay nada de DashOS, y se prueba en el PC con
//! `rustc --edition 2024 --test usuario/src/discos_modelo.rs`.
//!
//! Sustituye a la columna de iconos de la derecha (S10): un panel flotante translúcido, con las esquinas redondeadas,
//! como los widgets de escritorio de DankMaterialShell (`Modules/BuiltinDesktopPlugins/SystemMonitorWidget.qml`: el
//! fondo con `transparency`, las filas de los discos con su barra de 4 de alto, del color del acento, que pasa a aviso
//! y a error con el disco lleno; `Modules/ControlCenter/Details/DiskUsageDetail.qml`: icono, nombre, "usado / total" y
//! el porcentaje). Una tarjeta por volumen, con su icono de la colección, su nombre, lo libre y la barra del espacio.
//!
//! Las animaciones, con los muelles y las curvas de DMS (`muelle.rs`), y solo mientras algo se mueve:
//!
//! - **Aparecer**: el panel entra deslizándose desde su lado y se aclara (muelle "expresivo" de la duración media), y
//!   sus tarjetas entran tras él de una en una, como al montar.
//! - **Montar**: la tarjeta crece de alto desde 0 y se aclara, con el rebote del muelle expresivo; las de debajo se
//!   apartan con ella. **Desmontar**: lo contrario, sin rebote; al acabar se quita.
//! - **El espacio**: la barra se llena desde 0 al aparecer y se mueve hasta el valor nuevo cuando cambia.
//! - **El ratón encima**: la capa de estado de Material (8 %) se enciende, y el icono crece un poco.
//! - **Un clic**: la onda de Material desde donde se pulsó (450 ms, se apaga al final).
//!
//! Los valores que se animan van de 0 a 1000 (milésimas); el muelle puede pasarse un poco (el rebote).

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[cfg(test)]
#[path = "discos_ajustes.rs"]
pub mod ajustes;
#[cfg(test)]
#[path = "muelle.rs"]
pub mod muelle;
#[cfg(not(test))]
use crate::{ajustes, muelle};

use ajustes::{Altura, Discos, Lado, Tamano};
use muelle::{Curva, Muelle, Preset, UNO};

pub const MIL: i32 = 1000;
/// Tarjetas como mucho (y además, las que quepan en la pantalla).
pub const MAX_TARJETAS: usize = 24;
/// Lo que dura la onda de un clic.
pub const ONDA_US: u64 = 450_000;
/// Al aparecer el panel, lo que tarda en empezar a entrar la primera tarjeta y lo que espera cada una tras la de antes.
pub const RETRASO_PRIMERA_US: u64 = 120_000;
pub const RETRASO_CADA_US: u64 = 70_000;
/// Lo que se desliza el panel al aparecer.
pub const DESLIZ: i32 = 24;

/// Un volumen: su nombre (sin los dos puntos), su raíz, el icono de la colección y lo que mide y tiene libre (0 y 0 si
/// no se sabe).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volumen {
    pub nombre: String,
    pub raiz: u32,
    pub icono: &'static str,
    pub mide: u64,
    pub libre: u64,
}

/// Un rectángulo (sin el `Rect` del kit, para probarlo en el PC).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caja {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Caja {
    pub fn contiene(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// Las medidas del panel según su tamaño (en píxeles).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Medidas {
    pub ancho: i32,
    pub relleno: i32,
    pub cabecera: i32,
    pub tarjeta: i32,
    pub hueco: i32,
    pub icono: i32,
    /// El radio del panel y el de las tarjetas (`Theme.cornerRadius` de DMS, 12, y el de dentro, 12 - 2 como sus tiles).
    pub radio: i32,
    pub radio_tarjeta: i32,
    /// Lo que se separa del borde de la pantalla.
    pub margen: i32,
}

pub fn medidas(aj: &Discos) -> Medidas {
    let (ancho, tarjeta, icono) = match aj.tamano {
        Tamano::Pequeno => (208, 52, 32),
        Tamano::Mediano => (248, 60, 40),
        Tamano::Grande => (296, 72, 48),
    };
    // Sin la barra del espacio, la tarjeta es más baja (el nombre y lo libre).
    let tarjeta = if aj.espacio { tarjeta } else { tarjeta - 12 };
    Medidas { ancho, relleno: 8, cabecera: if aj.titulo { 32 } else { 0 }, tarjeta, hueco: 4, icono, radio: 16, radio_tarjeta: 12, margen: 12 }
}

/// Lo usado, en milésimas (0 a 1000). Con medidas que no cuadran (nada que medir, más libre que lo que mide), 0.
pub fn usado_milesimas(mide: u64, libre: u64) -> i32 {
    if mide == 0 || libre >= mide {
        return 0;
    }
    let usado = (mide - libre) as u128;
    ((usado * 1000) / mide as u128).min(1000) as i32
}

/// Un tamaño corto para la tarjeta: "512 KB", "1,5 MB", "12 GB" (con una cifra decimal por debajo de 10).
pub fn tamano_corto(b: u64) -> String {
    const UNIDADES: [&str; 6] = ["bytes", "KB", "MB", "GB", "TB", "PB"];
    if b < 1024 {
        return format!("{} bytes", b);
    }
    let mut u = 0;
    let mut v = b as u128;
    // En décimas de la unidad.
    let mut decimas = v * 10;
    while v >= 1024 && u < UNIDADES.len() - 1 {
        decimas = v * 10 / 1024;
        v /= 1024;
        u += 1;
    }
    if decimas < 100 && decimas % 10 != 0 { format!("{},{} {}", decimas / 10, decimas % 10, UNIDADES[u]) } else { format!("{} {}", (decimas + 5) / 10, UNIDADES[u]) }
}

/// En qué fase está una tarjeta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fase {
    Entra,
    Quieta,
    Sale,
}

#[derive(Clone, Debug)]
pub struct Tarjeta {
    pub vol: Volumen,
    pub fase: Fase,
    /// Lo que está (alto y opacidad), de 0 a 1000.
    pub presencia: Muelle,
    /// Lo usado que enseña la barra, de 0 a 1000.
    pub uso: Muelle,
    /// La capa del ratón encima, de 0 a 1000.
    pub encima: Muelle,
    /// La onda del último clic: dónde (dentro de la tarjeta) y cuándo.
    pub onda: Option<(i32, i32, u64)>,
    /// Lo que le falta para empezar a entrar (al aparecer el panel entran de una en una), en µs.
    pub retraso: Option<u64>,
}

impl Tarjeta {
    pub fn presente(&self) -> i32 {
        self.presencia.entero().clamp(0, MIL)
    }
}

/// Lo que cambió al poner los volúmenes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cambios {
    pub montados: Vec<String>,
    pub desmontados: Vec<String>,
    /// Los que no caben (más de `MAX_TARJETAS` o de los que caben en la pantalla).
    pub fuera: usize,
}

/// El panel de los discos.
#[derive(Clone, Debug)]
pub struct Panel {
    pub aj: Discos,
    pub tarjetas: Vec<Tarjeta>,
    /// La entrada del panel (deslizarse y aclararse), de 0 a 1000.
    pub entrada: Muelle,
    pub encima: Option<usize>,
    /// El elegido, por su nombre (sigue siéndolo aunque se muevan las tarjetas).
    pub elegido: Option<String>,
    /// El alto de la pantalla que se puede usar (para no salirse).
    pub alto_pantalla: i32,
    puesto: bool,
}

fn muelle_presencia(aj: &Discos, entra: bool) -> Muelle {
    let d = if aj.animaciones { muelle::media_ms(muelle::BASE_MS) * 2 } else { 0 };
    Muelle::preset(if entra { Preset::Expresivo } else { Preset::DeSerie }, d)
}

fn muelle_uso(aj: &Discos) -> Muelle {
    Muelle::preset(Preset::DeSerie, if aj.animaciones { 700 } else { 0 })
}

fn muelle_encima(aj: &Discos) -> Muelle {
    Muelle::preset(Preset::Rapido, if aj.animaciones { 150 } else { 0 })
}

/// Lleva un muelle a `v` (en milésimas).
fn ir(m: &mut Muelle, v: i32) {
    m.ir(v as i64 * UNO);
}

impl Panel {
    pub fn nuevo(aj: Discos, alto_pantalla: i32) -> Panel {
        let entrada = Muelle::preset(Preset::Expresivo, if aj.animaciones { muelle::media_ms(muelle::BASE_MS) * 2 } else { 0 });
        Panel { aj, tarjetas: Vec::new(), entrada, encima: None, elegido: None, alto_pantalla: alto_pantalla.max(0), puesto: false }
    }

    pub fn m(&self) -> Medidas {
        medidas(&self.aj)
    }

    /// Las tarjetas que caben en `alto_pantalla` (al menos una).
    pub fn caben(&self) -> usize {
        let m = self.m();
        let libre = self.alto_pantalla - 2 * m.margen - 2 * m.relleno - m.cabecera + m.hueco;
        ((libre / (m.tarjeta + m.hueco)).max(1) as usize).min(MAX_TARJETAS)
    }

    /// Ajustes nuevos: las medidas cambian al momento; los muelles, con su duración nueva.
    pub fn poner_ajustes(&mut self, aj: Discos) {
        if aj.animaciones != self.aj.animaciones {
            for t in &mut self.tarjetas {
                let (p, u, e) = (t.presencia, t.uso, t.encima);
                t.presencia = muelle_presencia(&aj, t.fase != Fase::Sale);
                t.presencia.saltar(p.destino);
                t.uso = muelle_uso(&aj);
                t.uso.saltar(u.destino);
                t.encima = muelle_encima(&aj);
                t.encima.saltar(e.destino);
            }
            let d = self.entrada.destino;
            self.entrada = Muelle::preset(Preset::Expresivo, if aj.animaciones { muelle::media_ms(muelle::BASE_MS) * 2 } else { 0 });
            self.entrada.saltar(d);
        }
        self.aj = aj;
    }

    /// Los volúmenes de ahora, en su orden. Los nuevos entran (la primera vez, sin animar cada uno: entra el panel
    /// entero); los que ya no están, salen. Nunca dos tarjetas con el mismo nombre.
    pub fn poner(&mut self, vols: &[Volumen]) -> Cambios {
        let mut cambios = Cambios::default();
        // Los que valen: sin nombre repetido, hasta los que caben.
        let caben = self.caben();
        let mut nuevos: Vec<&Volumen> = Vec::new();
        for v in vols {
            if v.nombre.is_empty() || nuevos.iter().any(|x| x.nombre == v.nombre) {
                continue;
            }
            if nuevos.len() >= caben {
                cambios.fuera += 1;
                continue;
            }
            nuevos.push(v);
        }
        let primera = !self.puesto;
        self.puesto = true;
        // Los que se van: a salir (siguen en su sitio hasta que acaban).
        for t in &mut self.tarjetas {
            let sigue = nuevos.iter().any(|v| v.nombre == t.vol.nombre);
            if !sigue && t.fase != Fase::Sale {
                t.fase = Fase::Sale;
                t.retraso = None;
                let v = t.presencia.valor;
                t.presencia = muelle_presencia(&self.aj, false);
                t.presencia.saltar(v);
                ir(&mut t.presencia, 0);
                t.onda = None;
                cambios.desmontados.push(t.vol.nombre.clone());
            }
        }
        // Los que siguen se ponen al día; los nuevos van tras el de antes en la lista nueva.
        let mut tras: Option<usize> = None;
        let mut nuevos_puestos = 0;
        for v in &nuevos {
            if let Some(k) = self.tarjetas.iter().position(|t| t.vol.nombre == v.nombre) {
                let t = &mut self.tarjetas[k];
                if t.fase == Fase::Sale {
                    // Vuelve antes de irse del todo.
                    t.fase = Fase::Entra;
                    let p = t.presencia.valor;
                    t.presencia = muelle_presencia(&self.aj, true);
                    t.presencia.saltar(p);
                    ir(&mut t.presencia, MIL);
                    cambios.montados.push(v.nombre.clone());
                }
                if t.vol.mide != v.mide || t.vol.libre != v.libre {
                    ir(&mut t.uso, usado_milesimas(v.mide, v.libre));
                }
                t.vol = (*v).clone();
                tras = Some(k);
                continue;
            }
            let mut t = Tarjeta {
                vol: (*v).clone(),
                fase: Fase::Entra,
                presencia: muelle_presencia(&self.aj, true),
                uso: muelle_uso(&self.aj),
                encima: muelle_encima(&self.aj),
                onda: None,
                retraso: None,
            };
            if primera {
                // Al aparecer el panel, las tarjetas entran de una en una, tras él.
                if self.aj.animaciones {
                    t.retraso = Some(RETRASO_PRIMERA_US + nuevos_puestos as u64 * RETRASO_CADA_US);
                    nuevos_puestos += 1;
                } else {
                    t.presencia.saltar(MIL as i64 * UNO);
                    t.fase = Fase::Quieta;
                }
            } else {
                ir(&mut t.presencia, MIL);
                cambios.montados.push(v.nombre.clone());
            }
            // La barra se llena desde 0.
            ir(&mut t.uso, usado_milesimas(v.mide, v.libre));
            let k = tras.map_or(0, |k| k + 1);
            self.tarjetas.insert(k, t);
            if let Some(e) = self.encima.as_mut()
                && *e >= k
            {
                *e += 1;
            }
            tras = Some(k);
        }
        if primera {
            ir(&mut self.entrada, MIL);
        }
        if self.elegido.as_ref().is_some_and(|e| !nuevos.iter().any(|v| &v.nombre == e)) {
            self.elegido = None;
        }
        cambios
    }

    /// El alto de una tarjeta ahora (con lo que está).
    fn alto_tarjeta(&self, t: &Tarjeta) -> i32 {
        let m = self.m();
        (m.tarjeta + m.hueco) * t.presente() / MIL
    }

    /// El alto que tiene que tener la superficie: con todas las tarjetas enteras, también las que salen. Solo cambia al
    /// cambiar las tarjetas (no a cada cuadro).
    pub fn alto_superficie(&self) -> i32 {
        let m = self.m();
        let n = self.tarjetas.len().max(1) as i32;
        2 * m.relleno + m.cabecera + n * (m.tarjeta + m.hueco) - m.hueco
    }

    /// El ancho de la superficie: el panel y lo que se desliza al entrar.
    pub fn ancho_superficie(&self) -> i32 {
        self.m().ancho
    }

    /// El alto del panel ahora (con las tarjetas a medias).
    pub fn alto_panel(&self) -> i32 {
        let m = self.m();
        let n: i32 = self.tarjetas.iter().map(|t| self.alto_tarjeta(t)).sum();
        (2 * m.relleno + m.cabecera + n - m.hueco).max(2 * m.relleno + m.cabecera)
    }

    /// Dónde está el panel dentro de la superficie ahora (pegado a su borde según la altura, y deslizado al entrar).
    pub fn caja_panel(&self) -> Caja {
        let (w, h) = (self.m().ancho, self.alto_panel());
        let hs = self.alto_superficie();
        let y = match self.aj.altura {
            Altura::Arriba => 0,
            Altura::Centro => (hs - h) / 2,
            Altura::Abajo => hs - h,
        };
        let e = self.entrada.entero().clamp(0, MIL + 200);
        let corre = DESLIZ * (MIL - e.min(MIL)) / MIL;
        let x = match self.aj.lado {
            Lado::Derecha => corre,
            Lado::Izquierda => -corre,
        };
        Caja { x, y, w, h }
    }

    /// Lo que se ve del panel entero (de 0 a 255), por su entrada.
    pub fn opacidad_panel(&self) -> u32 {
        (self.entrada.entero().clamp(0, MIL) as u32 * 255 / MIL as u32).min(255)
    }

    /// La caja de cada tarjeta ahora (las que salen, también, mientras se vean).
    pub fn cajas(&self) -> Vec<(usize, Caja)> {
        let m = self.m();
        let p = self.caja_panel();
        let mut y = p.y + m.relleno + m.cabecera;
        let mut v = Vec::with_capacity(self.tarjetas.len());
        for (k, t) in self.tarjetas.iter().enumerate() {
            let alto = self.alto_tarjeta(t);
            if alto > 0 {
                v.push((k, Caja { x: p.x + m.relleno, y, w: m.ancho - 2 * m.relleno, h: (alto - m.hueco).max(0).min(m.tarjeta) }));
            }
            y += alto;
        }
        v
    }

    /// La caja de la tarjeta `k` cuando todo acabe de moverse (para decir dónde pulsar).
    pub fn caja_final(&self, k: usize) -> Option<Caja> {
        let m = self.m();
        let quedan: Vec<usize> = (0..self.tarjetas.len()).filter(|&i| self.tarjetas[i].fase != Fase::Sale).collect();
        let i = quedan.iter().position(|&i| i == k)?;
        let n = quedan.len() as i32;
        let hs = self.alto_superficie();
        let h = 2 * m.relleno + m.cabecera + n * (m.tarjeta + m.hueco) - m.hueco;
        let y0 = match self.aj.altura {
            Altura::Arriba => 0,
            Altura::Centro => (hs - h) / 2,
            Altura::Abajo => hs - h,
        };
        Some(Caja { x: m.relleno, y: y0 + m.relleno + m.cabecera + i as i32 * (m.tarjeta + m.hueco), w: m.ancho - 2 * m.relleno, h: m.tarjeta })
    }

    /// La tarjeta bajo (`x`, `y`) de la superficie: solo las que están (no las que salen ni las que apenas han entrado).
    pub fn bajo(&self, x: i32, y: i32) -> Option<usize> {
        self.cajas().into_iter().find(|(k, c)| c.contiene(x, y) && self.tarjetas[*k].fase != Fase::Sale && self.tarjetas[*k].presente() > MIL / 2).map(|(k, _)| k)
    }

    /// El ratón está sobre la tarjeta `k` (o sobre ninguna).
    pub fn poner_encima(&mut self, k: Option<usize>) -> bool {
        let k = k.filter(|&k| k < self.tarjetas.len());
        if k == self.encima {
            return false;
        }
        if let Some(t) = self.encima.and_then(|e| self.tarjetas.get_mut(e)) {
            ir(&mut t.encima, 0);
        }
        if let Some(t) = k.and_then(|e| self.tarjetas.get_mut(e)) {
            ir(&mut t.encima, MIL);
        }
        self.encima = k;
        true
    }

    /// Un clic en la tarjeta `k`, en (`x`, `y`) de la superficie: la elige y suelta la onda.
    pub fn pulsar(&mut self, k: usize, x: i32, y: i32, ahora: u64) {
        let Some(c) = self.cajas().into_iter().find(|(i, _)| *i == k).map(|(_, c)| c) else { return };
        let Some(t) = self.tarjetas.get_mut(k) else { return };
        self.elegido = Some(t.vol.nombre.clone());
        if self.aj.animaciones {
            t.onda = Some(((x - c.x).clamp(0, c.w), (y - c.y).clamp(0, c.h), ahora));
        }
    }

    /// La onda de la tarjeta `k` ahora: centro (dentro de la tarjeta), radio y opacidad (0 a 255).
    pub fn onda(&self, k: usize, ahora: u64) -> Option<(i32, i32, i32, u32)> {
        let t = self.tarjetas.get(k)?;
        let (x, y, desde) = t.onda?;
        let pasado = ahora.saturating_sub(desde);
        if pasado >= ONDA_US {
            return None;
        }
        let m = self.m();
        let fin = (m.ancho * m.ancho + m.tarjeta * m.tarjeta).isqrt_i32();
        let f = (pasado as i64 * UNO / ONDA_US as i64) as i64;
        let r = (fin as i64 * Curva::SalidaCubica.en(f) / UNO) as i32;
        // 12 % (la capa de pulsar de Material) que se apaga en la segunda mitad.
        let alfa = if f < UNO / 2 { 31 } else { (31 * (UNO - f) * 2 / UNO) as u32 };
        Some((x, y, r, alfa as u32))
    }

    /// Avanza `paso_us`. Quita las que acabaron de salir y las ondas acabadas. Si algo se movió.
    pub fn avanzar(&mut self, ahora: u64, paso_us: u64) -> bool {
        let mut movido = self.entrada.avanzar(paso_us);
        for t in &mut self.tarjetas {
            if let Some(r) = t.retraso {
                if r <= paso_us {
                    t.retraso = None;
                    ir(&mut t.presencia, MIL);
                    movido = true;
                } else {
                    t.retraso = Some(r - paso_us);
                }
            }
            movido |= t.presencia.avanzar(paso_us);
            movido |= t.uso.avanzar(paso_us);
            movido |= t.encima.avanzar(paso_us);
            if t.fase == Fase::Entra && !t.presencia.en_marcha && t.retraso.is_none() {
                t.fase = Fase::Quieta;
            }
            if let Some((_, _, desde)) = t.onda {
                // La onda se pinta también en el cuadro en que acaba (para borrarla).
                movido = true;
                if ahora.saturating_sub(desde) >= ONDA_US {
                    t.onda = None;
                }
            }
        }
        let antes = self.tarjetas.len();
        let encima = self.encima.and_then(|e| self.tarjetas.get(e)).map(|t| t.vol.nombre.clone());
        self.tarjetas.retain(|t| !(t.fase == Fase::Sale && !t.presencia.en_marcha && t.presencia.valor <= UNO / 2));
        if self.tarjetas.len() != antes {
            movido = true;
            self.encima = encima.and_then(|n| self.tarjetas.iter().position(|t| t.vol.nombre == n));
        }
        movido
    }

    /// Si algo se mueve (y hay que pintar el cuadro siguiente).
    pub fn animando(&self) -> bool {
        self.entrada.en_marcha || self.tarjetas.iter().any(|t| t.presencia.en_marcha || t.uso.en_marcha || t.encima.en_marcha || t.onda.is_some() || t.retraso.is_some())
    }
}

trait RaizEntera {
    fn isqrt_i32(self) -> i32;
}

impl RaizEntera for i32 {
    fn isqrt_i32(self) -> i32 {
        if self <= 0 {
            return 0;
        }
        let (mut x, mut y) = (self as i64, (self as i64 + 1) / 2);
        while y < x {
            x = y;
            y = (x + self as i64 / x) / 2;
        }
        x as i32
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::string::ToString;

    fn vol(n: &str, mide: u64, libre: u64) -> Volumen {
        Volumen { nombre: n.to_string(), raiz: 1, icono: "disco-duro", mide, libre }
    }

    fn de_siempre() -> Vec<Volumen> {
        alloc::vec![vol("SYS", 1 << 30, 600 << 20), vol("Home", 1 << 30, 600 << 20), vol("RAM", 64 << 20, 60 << 20), vol("Work", 1 << 30, 600 << 20)]
    }

    /// Avanza hasta que todo se para (o 20 s de cuadros): el tiempo que pasó.
    fn asentar(p: &mut Panel, ahora: &mut u64) -> u64 {
        let desde = *ahora;
        for _ in 0..1200 {
            *ahora += 16_667;
            p.avanzar(*ahora, 16_667);
            if !p.animando() {
                break;
            }
        }
        *ahora - desde
    }

    #[test]
    fn medidas_y_tamanos() {
        assert_eq!(usado_milesimas(0, 0), 0);
        assert_eq!(usado_milesimas(100, 200), 0);
        assert_eq!(usado_milesimas(100, 25), 750);
        assert_eq!(usado_milesimas(u64::MAX, 0), 1000);
        assert_eq!(usado_milesimas(u64::MAX, 1), 999);
        assert_eq!(tamano_corto(0), "0 bytes");
        assert_eq!(tamano_corto(1023), "1023 bytes");
        assert_eq!(tamano_corto(1024), "1 KB");
        assert_eq!(tamano_corto(1536 << 10), "1,5 MB");
        assert_eq!(tamano_corto(12 << 30), "12 GB");
        assert_eq!(tamano_corto(u64::MAX), "16384 PB");
    }

    #[test]
    fn la_primera_vez_entra_el_panel_y_se_llenan_las_barras() {
        let mut p = Panel::nuevo(Discos::de_serie(), 560);
        let c = p.poner(&de_siempre());
        assert!(c.montados.is_empty() && c.desmontados.is_empty());
        assert_eq!(p.tarjetas.len(), 4);
        assert!(p.animando());
        assert!(p.opacidad_panel() < 10);
        // Entran de una en una: a los 200 ms, la primera ya asoma y la última aún no.
        let mut q = p.clone();
        let mut t = 0;
        for _ in 0..12 {
            t += 16_667;
            q.avanzar(t, 16_667);
        }
        assert!(q.tarjetas[0].presente() > 0 && q.tarjetas[3].presente() == 0, "{} {}", q.tarjetas[0].presente(), q.tarjetas[3].presente());
        assert_eq!(p.tarjetas[0].uso.entero(), 0);
        let mut t = 0;
        let dura = asentar(&mut p, &mut t);
        assert!(dura < 2_000_000, "tarda {} us", dura);
        assert_eq!(p.opacidad_panel(), 255);
        assert_eq!(p.caja_panel().x, 0);
        assert!((p.tarjetas[0].uso.entero() - 414).abs() <= 1);
        // Quieto, no se mueve nada ni pide cuadros (sin gastar en reposo).
        assert!(!p.animando());
        assert!(!p.avanzar(t + 16_667, 16_667));
        // Las cajas y las finales coinciden, en orden, sin pisarse.
        let cajas = p.cajas();
        for (k, c) in &cajas {
            assert_eq!(Some(*c), p.caja_final(*k));
        }
        for w in cajas.windows(2) {
            assert!(w[0].1.y + w[0].1.h <= w[1].1.y);
        }
        assert_eq!(p.alto_panel(), p.alto_superficie());
    }

    #[test]
    fn montar_crece_con_rebote_y_desmontar_se_va() {
        let mut p = Panel::nuevo(Discos::de_serie(), 560);
        let mut t = 0;
        p.poner(&de_siempre()[..2]);
        asentar(&mut p, &mut t);
        let alto = p.alto_panel();
        let c = p.poner(&de_siempre());
        assert_eq!(c.montados, ["RAM", "Work"]);
        assert_eq!(p.tarjetas.iter().map(|t| t.vol.nombre.as_str()).collect::<Vec<_>>(), ["SYS", "Home", "RAM", "Work"]);
        // La superficie crece ya; el panel, poco a poco y con rebote (el muelle expresivo se pasa).
        assert!(p.alto_superficie() > alto);
        assert_eq!(p.alto_panel(), alto);
        let mut maximo = 0;
        for _ in 0..200 {
            t += 16_667;
            p.avanzar(t, 16_667);
            maximo = maximo.max(p.tarjetas[2].presencia.entero());
        }
        assert!(maximo > MIL, "sin rebote: {}", maximo);
        asentar(&mut p, &mut t);
        assert_eq!(p.alto_panel(), p.alto_superficie());
        // Desmontar: sigue en su sitio mientras sale; la superficie no cambia hasta que se quita.
        let hs = p.alto_superficie();
        let c = p.poner(&[vol("SYS", 1, 0), vol("Work", 1, 0)]);
        assert_eq!(c.desmontados, ["Home", "RAM"]);
        assert_eq!(p.alto_superficie(), hs);
        assert!(p.bajo(20, p.cajas()[1].1.y + 5).is_none(), "una que sale no se pulsa");
        asentar(&mut p, &mut t);
        assert_eq!(p.tarjetas.len(), 2);
        assert!(p.alto_superficie() < hs);
        assert_eq!(p.alto_panel(), p.alto_superficie());
    }

    #[test]
    fn poner_otra_vez_mientras_entran_no_las_deja_a_medias() {
        // Lo que hace el Workbench: pone los volúmenes al aparecer y otra vez al rato (la sesión, las medidas).
        let mut p = Panel::nuevo(Discos::de_serie(), 560);
        let mut t = 0;
        p.poner(&de_siempre()[..2]);
        p.avanzar(16_667, 16_667);
        p.poner(&de_siempre()[..2]);
        p.poner(&de_siempre());
        asentar(&mut p, &mut t);
        assert!(p.tarjetas.iter().all(|t| t.presente() == MIL && t.fase == Fase::Quieta), "{:?}", p.tarjetas.iter().map(|t| t.presente()).collect::<Vec<_>>());
        assert!(p.bajo(20, p.cajas()[0].1.y + 5).is_some());
    }

    #[test]
    fn vuelve_antes_de_irse() {
        let mut p = Panel::nuevo(Discos::de_serie(), 560);
        let mut t = 0;
        p.poner(&de_siempre());
        asentar(&mut p, &mut t);
        p.poner(&de_siempre()[..1]);
        for _ in 0..5 {
            t += 16_667;
            p.avanzar(t, 16_667);
        }
        let c = p.poner(&de_siempre());
        assert_eq!(c.montados.len(), 3);
        assert_eq!(p.tarjetas.len(), 4, "sin repetidas");
        asentar(&mut p, &mut t);
        assert!(p.tarjetas.iter().all(|t| t.fase == Fase::Quieta && t.presente() == MIL));
    }

    #[test]
    fn el_raton_encima_y_la_onda() {
        let mut p = Panel::nuevo(Discos::de_serie(), 560);
        let mut t = 0;
        p.poner(&de_siempre());
        asentar(&mut p, &mut t);
        let c = p.cajas()[1].1;
        let k = p.bajo(c.x + 10, c.y + 10).unwrap();
        assert_eq!(k, 1);
        assert!(p.poner_encima(Some(k)));
        assert!(!p.poner_encima(Some(k)), "lo mismo no cambia nada");
        assert!(p.animando());
        asentar(&mut p, &mut t);
        assert_eq!(p.tarjetas[1].encima.entero(), MIL);
        p.pulsar(k, c.x + 30, c.y + 20, t);
        assert_eq!(p.elegido.as_deref(), Some("Home"));
        let (x, y, r, a) = p.onda(k, t + 100_000).unwrap();
        assert_eq!((x, y), (30, 20));
        assert!(r > 0 && a > 0);
        assert!(p.onda(k, t + ONDA_US).is_none());
        let dura = asentar(&mut p, &mut t);
        assert!(dura <= ONDA_US + 40_000);
        assert!(p.tarjetas[1].onda.is_none());
        p.poner_encima(None);
        asentar(&mut p, &mut t);
        assert_eq!(p.tarjetas[1].encima.entero(), 0);
        // Fuera de todo: nada.
        for (x, y) in [(-5, -5), (i32::MIN, i32::MAX), (i32::MAX, 0), (p.m().ancho + 1, 30)] {
            assert!(p.bajo(x, y).is_none());
        }
    }

    #[test]
    fn sin_animaciones_salta_todo() {
        let mut aj = Discos::de_serie();
        aj.animaciones = false;
        let mut p = Panel::nuevo(aj, 560);
        p.poner(&de_siempre());
        p.avanzar(1, 16_667);
        assert!(!p.animando());
        assert_eq!(p.opacidad_panel(), 255);
        p.poner(&de_siempre()[..2]);
        p.avanzar(2, 16_667);
        assert_eq!(p.tarjetas.len(), 2);
        assert!(!p.animando());
        p.pulsar(0, 10, 10, 3);
        assert!(p.tarjetas[0].onda.is_none());
    }

    #[test]
    fn los_lados_y_las_alturas() {
        for altura in [Altura::Arriba, Altura::Centro, Altura::Abajo] {
            for lado in [Lado::Derecha, Lado::Izquierda] {
                for tamano in [Tamano::Pequeno, Tamano::Mediano, Tamano::Grande] {
                    let aj = Discos { altura, lado, tamano, ..Discos::de_serie() };
                    let mut p = Panel::nuevo(aj, 1080);
                    let mut t = 0;
                    p.poner(&de_siempre());
                    asentar(&mut p, &mut t);
                    let c = p.caja_panel();
                    assert_eq!((c.x, c.y, c.h), (0, 0, p.alto_superficie()));
                    p.poner(&de_siempre()[..1]);
                    asentar(&mut p, &mut t);
                    let c = p.caja_panel();
                    assert!(c.y >= 0 && c.y + c.h <= p.alto_superficie());
                    // Mientras entra, se desliza desde su lado.
                    let mut q = Panel::nuevo(Discos { altura, lado, tamano, ..Discos::de_serie() }, 1080);
                    q.poner(&de_siempre());
                    let x = q.caja_panel().x;
                    assert!(if lado == Lado::Derecha { x > 0 } else { x < 0 });
                }
            }
        }
    }

    #[test]
    fn hostil_mil_volumenes_y_pantallas_absurdas() {
        let mil: Vec<Volumen> = (0..1000).map(|i| vol(&format!("V{}", i), i as u64 * 1000, i as u64 * 7)).collect();
        for alto in [0, 1, 50, 600, 1080, 100_000] {
            let mut p = Panel::nuevo(Discos::de_serie(), alto);
            let c = p.poner(&mil);
            assert!(p.tarjetas.len() <= MAX_TARJETAS && !p.tarjetas.is_empty());
            assert_eq!(c.fuera, 1000 - p.tarjetas.len());
            if alto >= 600 {
                assert!(p.alto_superficie() + 2 * p.m().margen <= alto, "se sale: {} de {}", p.alto_superficie(), alto);
            }
        }
        // Nombres repetidos y vacíos: uno de cada.
        let mut p = Panel::nuevo(Discos::de_serie(), 1080);
        p.poner(&[vol("A", 1, 1), vol("A", 2, 2), vol("", 3, 3), vol("B", 0, 99)]);
        assert_eq!(p.tarjetas.len(), 2);
        assert_eq!(p.tarjetas[1].uso.destino, 0);
    }

    #[test]
    fn hostil_rafagas_de_montar_y_desmontar() {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut azar = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let nombres: Vec<String> = (0..30).map(|i| format!("D{}", i)).collect();
        let mut p = Panel::nuevo(Discos::de_serie(), 1080);
        let mut t = 0u64;
        for vuelta in 0..10_000 {
            let n = (azar() % 12) as usize;
            let vols: Vec<Volumen> = (0..n).map(|_| vol(&nombres[(azar() % 30) as usize], azar() % (1 << 40), azar() % (1 << 41))).collect();
            p.poner(&vols);
            // Unas veces en la misma tanda, otras con cuadros (también pasos absurdos) y el ratón y clics por medio.
            if vuelta % 3 == 0 {
                let paso = [0, 1, 16_667, 33_333, u64::MAX / 4][(azar() % 5) as usize];
                t = t.saturating_add(paso.min(1_000_000));
                p.avanzar(t, paso);
            }
            let (mx, my) = ((azar() % 400) as i32 - 50, (azar() % 800) as i32 - 50);
            let b = p.bajo(mx, my);
            p.poner_encima(b);
            if let Some(k) = b {
                p.pulsar(k, mx, my, t);
            }
            // Nunca dos tarjetas vivas con el mismo nombre, y no más de las que caben más las que salen.
            let vivas: Vec<&str> = p.tarjetas.iter().filter(|t| t.fase != Fase::Sale).map(|t| t.vol.nombre.as_str()).collect();
            for (i, a) in vivas.iter().enumerate() {
                assert!(!vivas[i + 1..].contains(a));
            }
            assert!(vivas.len() <= MAX_TARJETAS);
            assert!(p.tarjetas.len() <= 30);
            assert!(p.encima.is_none_or(|e| e < p.tarjetas.len()));
            for t in &p.tarjetas {
                assert!((0..=MIL as i64).contains(&(t.uso.destino / UNO)));
            }
        }
        // Al final se para y quedan solo las vivas.
        asentar(&mut p, &mut t);
        assert!(!p.animando());
        assert!(p.tarjetas.iter().all(|t| t.fase == Fase::Quieta));
        assert_eq!(p.alto_panel(), p.alto_superficie());
    }

    #[test]
    fn cambiar_los_ajustes_en_marcha() {
        let mut p = Panel::nuevo(Discos::de_serie(), 1080);
        let mut t = 0;
        p.poner(&de_siempre());
        asentar(&mut p, &mut t);
        let mut aj = Discos::de_serie();
        aj.tamano = Tamano::Grande;
        aj.espacio = false;
        aj.titulo = false;
        p.poner_ajustes(aj.clone());
        assert_eq!(p.m().ancho, 296);
        assert_eq!(p.m().tarjeta, 60);
        assert_eq!(p.alto_panel(), p.alto_superficie());
        aj.animaciones = false;
        p.poner_ajustes(aj);
        p.poner(&de_siempre()[..1]);
        p.avanzar(t + 1, 16_667);
        assert!(!p.animando());
        assert_eq!(p.tarjetas.len(), 1);
    }
}
