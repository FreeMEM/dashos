//! S27: el dock, sin pintar: qué lleva (programas fijados y ventanas abiertas, agrupadas o no, el lanzador, el
//! separador y el botón de los que no caben), dónde va cada cosa, cuándo se ve y cómo se mueve.
//!
//! Portado de DankMaterialShell v2.1.2: `Modules/SurfaceWidgets/ApplicationStrip.qml` (`buildGroupedItems`,
//! `buildUngroupedItems`, `insertLauncher`), `ApplicationModel.js` (`overflow`, `pinnedItem`, `separator`),
//! `ApplicationItem.qml` (las medidas de cada cosa), `Modules/Dock/DockBody.qml` y `DockGeometry.qml` (el cuerpo, la
//! capa, la zona exclusiva, cuándo se ve, el deslizamiento al esconderse), `DockAppButton.qml` (el indicador, la
//! letra cuando no hay icono), `DockHoverBounce.qml` (el saltito bajo el ratón) y `DockContextMenu.qml` (el menú del
//! botón derecho). Copyright (c) 2025 Avenge Media LLC, licencia MIT (`docs/licencias/DankMaterialShell-MIT.txt`).
//!
//! Todo en píxeles enteros y en un eje: "a lo largo" del borde (x si está abajo o arriba) y "de través" (y). Las
//! medidas que DMS saca de su tema (`Theme.spacingS`...) van aquí con su nombre.
//!
//! Sin nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/dock_modelo.rs`.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// En el PC (las pruebas) van aquí; en el programa, los declara él (`crate::ajustes`, `crate::muelle`).
#[cfg(test)]
#[path = "dock_ajustes.rs"]
pub mod ajustes;
#[cfg(test)]
#[path = "muelle.rs"]
pub mod muelle;
#[cfg(not(test))]
use crate::{ajustes, muelle};

use ajustes::{Borde, Dock, Indicador, Modo};
use muelle::{Curva, Muelle, Preset, Tramo, UNO};

/// `Theme.spacingXXS`, `spacingXS`, `spacingS`, `spacingM` (`Common/Theme.qml`).
pub const ESPACIO_XXS: i32 = 2;
pub const ESPACIO_XS: i32 = 4;
pub const ESPACIO_S: i32 = 8;
pub const ESPACIO_M: i32 = 12;
/// El separador entre fijados y abiertos: 8 a lo largo (`ApplicationItem.primarySize`), una raya de 2 por la mitad
/// del icono.
pub const SEPARADOR: i32 = 8;
/// Ventanas como mucho en el dock: el compositor no da más (`wayland::dock`).
pub const MAX_VENTANAS: usize = 128;
/// Puntos del indicador de un grupo, como mucho (`Math.min(windowCount, 4)`).
pub const MAX_PUNTOS: usize = 4;
/// Lo que espera el dock tras salir el ratón antes de esconderse (`revealHold`), y la burbuja antes de salir
/// (`tooltipRevealDelay`), y la entrada al arrancar (`startupRevealTimer`).
pub const ESPERA_ESCONDER_US: u64 = 250_000;
pub const ESPERA_BURBUJA_US: u64 = 250_000;
pub const ESPERA_ARRANQUE_US: u64 = 200_000;

/// Una ventana abierta, como la da el compositor (`dashos_dock_v1.ventana`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ventana {
    pub id: u32,
    /// El programa (su nombre en `Programas/`, o "linux:foot" para los de las cajas).
    pub programa: String,
    pub titulo: String,
    pub espacio: u32,
    pub salida: u32,
    /// Si tiene el teclado.
    pub activa: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Lanzador,
    /// Fijado y sin ventanas (o, sin agrupar, fijado con las suyas aparte).
    Fijado,
    /// Una ventana (sin agrupar).
    Ventana,
    /// Un programa con sus ventanas (agrupado), fijado o no.
    Grupo,
    Separador,
    /// El botón de los que no caben (`overflow-toggle`).
    Desbordado,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Elemento {
    pub tipo: Tipo,
    pub programa: String,
    pub fijado: bool,
    /// `isRunning`: tiene alguna ventana (o, fijado sin agrupar, hay ventanas de él aparte).
    pub abierto: bool,
    /// Sus ventanas (número y si tiene el teclado), en el orden del compositor.
    pub ventanas: Vec<(u32, bool)>,
    pub en_desbordado: bool,
    /// Los que no caben (solo el botón).
    pub cuantos: usize,
    /// `uniqueKey`: lo que lo identifica entre una lista y la siguiente.
    pub clave: String,
}

impl Elemento {
    fn nuevo(tipo: Tipo, programa: &str, clave: String) -> Elemento {
        Elemento { tipo, programa: programa.to_string(), fijado: false, abierto: false, ventanas: Vec::new(), en_desbordado: false, cuantos: 0, clave }
    }

    /// Si alguna de sus ventanas tiene el teclado (`isWindowFocused`).
    pub fn activo(&self) -> bool {
        self.ventanas.iter().any(|v| v.1)
    }

    /// `shouldShowIndicator`.
    pub fn con_indicador(&self) -> bool {
        match self.tipo {
            Tipo::Ventana => true,
            Tipo::Grupo => !self.ventanas.is_empty(),
            Tipo::Fijado => self.abierto,
            _ => false,
        }
    }

    /// Los puntos del indicador (cuántos, y cuál está activo): un grupo, uno por ventana (hasta 4); lo demás, uno.
    pub fn puntos(&self) -> Vec<bool> {
        if !self.con_indicador() {
            return Vec::new();
        }
        if self.tipo == Tipo::Grupo && self.ventanas.len() > 1 {
            return self.ventanas.iter().take(MAX_PUNTOS).map(|v| v.1).collect();
        }
        alloc::vec![self.activo()]
    }

    /// Si se puede fijar o quitar (`hidePin`: ni el lanzador, ni el separador, ni el botón).
    pub fn fijable(&self) -> bool {
        matches!(self.tipo, Tipo::Fijado | Tipo::Ventana | Tipo::Grupo)
    }
}

/// `buildBaseItems` + `overflow`: lo que lleva el dock, con estos ajustes y estas ventanas. Con `salida`, solo las
/// ventanas de esa salida (`isolateDisplays`).
pub fn construir(aj: &Dock, ventanas: &[Ventana], salida: Option<u32>) -> Vec<Elemento> {
    let ventanas: Vec<&Ventana> = ventanas.iter().filter(|v| salida.is_none_or(|s| v.salida == s)).take(MAX_VENTANAS).collect();
    let items = if aj.agrupar { agrupados(aj, &ventanas) } else { sin_agrupar(aj, &ventanas) };
    desbordar(items, aj.max_fijados as usize, aj.max_abiertos as usize)
}

fn lanzador(aj: &Dock, items: &mut Vec<Elemento>) {
    if aj.lanzador {
        let mut l = Elemento::nuevo(Tipo::Lanzador, "", "launcher_button".to_string());
        l.fijado = true;
        items.insert(0, l);
    }
}

fn separador(clave: &str) -> Elemento {
    Elemento::nuevo(Tipo::Separador, "", clave.to_string())
}

fn fijado(programa: &str, abierto: bool) -> Elemento {
    let mut e = Elemento::nuevo(Tipo::Fijado, programa, alloc::format!("pinned_{}", programa));
    e.fijado = true;
    e.abierto = abierto;
    e
}

fn agrupados(aj: &Dock, ventanas: &[&Ventana]) -> Vec<Elemento> {
    let mut grupos: Vec<Elemento> = Vec::new();
    let separar = aj.separar_fijados;
    if !separar {
        for p in &aj.fijados {
            let mut g = Elemento::nuevo(Tipo::Grupo, p, alloc::format!("grouped_{}", p));
            g.fijado = true;
            grupos.push(g);
        }
    }
    for v in ventanas {
        let i = match grupos.iter().position(|g| g.programa == v.programa) {
            Some(i) => i,
            None => {
                grupos.push(Elemento::nuevo(Tipo::Grupo, &v.programa, alloc::format!("grouped_{}", v.programa)));
                grupos.len() - 1
            }
        };
        grupos[i].ventanas.push((v.id, v.activa));
        grupos[i].abierto = true;
    }
    let (mut fijados, sueltos): (Vec<Elemento>, Vec<Elemento>) = grupos.into_iter().partition(|g| g.fijado);
    if separar {
        for p in &aj.fijados {
            fijados.push(fijado(p, false));
        }
    }
    let mut items = fijados;
    lanzador(aj, &mut items);
    let hay_fijados = items.iter().any(|e| e.fijado && e.tipo != Tipo::Lanzador);
    if hay_fijados && !sueltos.is_empty() {
        items.push(separador("separator_grouped"));
    }
    items.extend(sueltos);
    items
}

fn sin_agrupar(aj: &Dock, ventanas: &[&Ventana]) -> Vec<Elemento> {
    let mut restantes: Vec<Elemento> = ventanas
        .iter()
        .map(|v| {
            let mut e = Elemento::nuevo(Tipo::Ventana, &v.programa, alloc::format!("window_{}", v.id));
            e.abierto = true;
            e.ventanas.push((v.id, v.activa));
            e
        })
        .collect();
    let mut items = Vec::new();
    for p in &aj.fijados {
        if aj.separar_fijados {
            items.push(fijado(p, false));
            continue;
        }
        match restantes.iter().position(|e| &e.programa == p) {
            Some(i) => {
                let mut e = restantes.remove(i);
                e.fijado = true;
                e.clave = alloc::format!("pinned_{}", p);
                items.push(e);
            }
            None => {
                let abierto = ventanas.iter().any(|v| &v.programa == p);
                items.push(fijado(p, abierto));
            }
        }
    }
    lanzador(aj, &mut items);
    if !aj.fijados.is_empty() && !restantes.is_empty() {
        items.push(separador("separator_ungrouped"));
    }
    items.extend(restantes);
    items
}

/// `ApplicationModel.overflow`: los fijados de más de `max_fijados` y los abiertos de más de `max_abiertos` (0, sin
/// límite) van al desbordado, tras un botón con su cuenta.
fn desbordar(items: Vec<Elemento>, max_fijados: usize, max_abiertos: usize) -> Vec<Elemento> {
    let fijados: Vec<String> = items.iter().filter(|e| e.fijado && e.tipo != Tipo::Lanzador).map(|e| e.clave.clone()).collect();
    let abiertos: Vec<String> = items.iter().filter(|e| e.abierto && !e.fijado).map(|e| e.clave.clone()).collect();
    let mut ocultos: Vec<String> = Vec::new();
    if max_fijados > 0 {
        ocultos.extend(fijados.iter().skip(max_fijados).cloned());
    }
    if max_abiertos > 0 {
        ocultos.extend(abiertos.iter().skip(max_abiertos).cloned());
    }
    if ocultos.is_empty() {
        return items;
    }
    let mut r: Vec<Elemento> = Vec::new();
    let mut separado = false;
    for mut e in items {
        match e.tipo {
            Tipo::Separador => continue,
            Tipo::Lanzador => {
                r.push(e);
                continue;
            }
            _ => {}
        }
        if !e.fijado && !e.abierto {
            continue;
        }
        if !e.fijado && !separado && !r.is_empty() {
            r.push(separador("separator_overflow"));
            separado = true;
        }
        e.en_desbordado = ocultos.contains(&e.clave);
        r.push(e);
    }
    let donde = r.iter().position(|e| e.tipo == Tipo::Separador).unwrap_or(r.len());
    let mut boton = Elemento::nuevo(Tipo::Desbordado, "", "overflow_toggle".to_string());
    boton.cuantos = ocultos.len();
    r.insert(donde, boton);
    r
}

/// Un rectángulo (en la capa del dock o en la pantalla).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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

/// Lo que se pide al compositor para la capa del dock (wlr-layer-shell).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capa {
    pub ancho: i32,
    pub alto: i32,
    /// Anclas: 1 arriba, 2 abajo, 4 izquierda, 8 derecha.
    pub anclas: u32,
    /// Zona exclusiva: -1 (no aparta las ventanas ni se aparta de las barras) o lo que reserva.
    pub zona: i32,
}

/// Dónde va todo, en la capa: su tamaño, el cuerpo, la celda de cada elemento (vacía si está en el desbordado y
/// cerrado) y lo que recibe el ratón visible y escondido.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disposicion {
    pub capa: Capa,
    pub cuerpo: Caja,
    pub celdas: Vec<Caja>,
    pub entrada_vista: Caja,
    pub entrada_escondida: Caja,
    /// Cuánto se aparta el dock al esconderse (hacia su borde).
    pub escondido: i32,
    /// El hueco para el saltito de los iconos (`animationHeadroom`).
    pub cabecera: i32,
}

impl Disposicion {
    /// El elemento bajo el punto (en la capa, con el dock en su sitio).
    pub fn en(&self, x: i32, y: i32) -> Option<usize> {
        self.celdas.iter().position(|c| c.w > 0 && c.contiene(x, y))
    }
}

/// Lo que mide un elemento a lo largo: el separador, 8; el resto, el icono (`ApplicationItem.primarySize`).
fn largo(aj: &Dock, e: &Elemento, desplegado: bool) -> i32 {
    if e.en_desbordado && !desplegado {
        return 0;
    }
    if e.tipo == Tipo::Separador { SEPARADOR } else { aj.iconos }
}

/// `widgetStrip.restLength`: lo que miden en fila, con la separación entre los que se ven.
pub fn largo_fila(aj: &Dock, items: &[Elemento], desplegado: bool) -> i32 {
    let vistos: Vec<i32> = items.iter().map(|e| largo(aj, e, desplegado)).filter(|&l| l > 0).collect();
    vistos.iter().sum::<i32>() + aj.separacion * (vistos.len() as i32 - 1).max(0)
}

/// `DockGeometry` y `DockBody`: la capa, el cuerpo y las celdas, para un monitor de `pantalla` (ancho, alto).
pub fn disponer(aj: &Dock, items: &[Elemento], desplegado: bool, pantalla: (i32, i32)) -> Disposicion {
    let vertical = aj.posicion.vertical();
    let (largo_pantalla, _) = if vertical { (pantalla.1, pantalla.0) } else { pantalla };
    let compacto = aj.modo == Modo::Compacto;
    let borde = if aj.borde { aj.grosor_borde } else { 0 };
    let grosor = aj.grosor();
    let cabecera = (aj.iconos * 35 + 99) / 100;
    // `effectiveMargin` y `bodyEdgeMargin` (sin marco ni barra pegada); en modo barra, sin margen.
    let margen = if compacto { aj.margen } else { 0 } + borde;
    let superficie = margen + grosor;
    let a_traves = superficie + (aj.iconos * 3 + 5) / 10 + cabecera;
    // `availablePrimary`.
    let disponible = (largo_pantalla - (if compacto { aj.margen } else { 0 } + borde) * 2).max(0);
    let fila = largo_fila(aj, items, desplegado);
    let largo_cuerpo = if compacto { (fila + aj.relleno * 2).min(disponible) } else { disponible };
    // La capa: a lo largo, lo que recibe el ratón (`dockMouseArea`: el cuerpo y el relleno de los lados); en modo
    // barra, de lado a lado.
    let largo_capa = if compacto { (largo_cuerpo + aj.relleno * 2).min(largo_pantalla).max(1) } else { largo_pantalla.max(1) };
    let ancla_borde = match aj.posicion {
        Borde::Arriba => 1,
        Borde::Abajo => 2,
        Borde::Izquierda => 4,
        Borde::Derecha => 8,
    };
    let anclas = if compacto { ancla_borde } else if vertical { ancla_borde | 1 | 2 } else { ancla_borde | 4 | 8 };
    // `surfaceExclusiveZone`: reserva su sitio solo si no se esconde.
    let zona = if aj.activo && !aj.autoocultar && !aj.ocultar_con_ventanas { (grosor + margen + if compacto { aj.hueco } else { 0 }).max(0) } else { -1 };
    // En el eje: `along` empieza en 0; `cross` cuenta desde el borde de la pantalla hacia dentro.
    let al_inicio = (largo_capa - largo_cuerpo) / 2;
    let cuerpo_eje = (al_inicio, margen, largo_cuerpo, grosor);
    // Las celdas, en fila desde el relleno; en modo barra, según `alinear`.
    let mut pos = al_inicio + aj.relleno;
    if !compacto {
        let sobra = (largo_cuerpo - aj.relleno * 2 - fila).max(0);
        pos += match aj.alinear {
            ajustes::Alinear::Inicio => 0,
            ajustes::Alinear::Centro => sobra / 2,
            ajustes::Alinear::Fin => sobra,
        };
    }
    let mut celdas_eje = Vec::with_capacity(items.len());
    for e in items {
        let l = largo(aj, e, desplegado);
        if l == 0 {
            celdas_eje.push((pos, margen + aj.relleno, 0, 0));
            continue;
        }
        celdas_eje.push((pos, margen + aj.relleno, l, aj.iconos));
        pos += l + aj.separacion;
    }
    // Lo que recibe el ratón: visto, el cuerpo con su relleno y el hueco del saltito; escondido, una raya de 1 en el
    // borde de la pantalla, de lo que mide el cuerpo con su relleno (`dockMouseArea` con `reveal` falso).
    let vista_eje = (0, 0, largo_capa, (superficie + borde + cabecera).min(a_traves));
    let escondida_eje = (0, 0, largo_capa, 1);
    // Del eje a la capa: `cross` 0 es el borde de la pantalla.
    let a_capa = |(a, c, l, g): (i32, i32, i32, i32)| -> Caja {
        match aj.posicion {
            Borde::Abajo => Caja { x: a, y: a_traves - c - g, w: l, h: g },
            Borde::Arriba => Caja { x: a, y: c, w: l, h: g },
            Borde::Derecha => Caja { x: a_traves - c - g, y: a, w: g, h: l },
            Borde::Izquierda => Caja { x: c, y: a, w: g, h: l },
        }
    };
    let (ancho, alto) = if vertical { (a_traves, largo_capa) } else { (largo_capa, a_traves) };
    Disposicion {
        capa: Capa { ancho, alto, anclas, zona },
        cuerpo: a_capa(cuerpo_eje),
        celdas: celdas_eje.into_iter().map(a_capa).collect(),
        entrada_vista: a_capa(vista_eje),
        entrada_escondida: a_capa(escondida_eje),
        // `hideDistance`: `motionThickness` + 10.
        escondido: superficie + 10,
        cabecera,
    }
}

/// Hacia dónde se aparta el dock al esconderse (el signo del deslizamiento en x e y de la capa).
pub fn sentido_escondido(b: Borde) -> (i32, i32) {
    match b {
        Borde::Abajo => (0, 1),
        Borde::Arriba => (0, -1),
        Borde::Derecha => (1, 0),
        Borde::Izquierda => (-1, 0),
    }
}

/// El indicador de un elemento en su celda (`DockAppButton`: `rowIndicator`/`columnIndicator`): en el relleno, del
/// lado del borde de la pantalla. Cada punto: su caja y si está activo (el activo, tres veces más largo y del primario).
pub fn indicador(aj: &Dock, e: &Elemento, celda: Caja) -> Vec<(Caja, bool)> {
    if aj.indicador == Indicador::Ninguno || celda.w == 0 {
        return Vec::new();
    }
    let puntos = e.puntos();
    if puntos.is_empty() {
        return puntos.iter().map(|_| (Caja::default(), false)).collect();
    }
    let icono = aj.iconos;
    let grupo = e.tipo == Tipo::Grupo && e.ventanas.len() > 1;
    let (base, grueso) = match aj.indicador {
        Indicador::Circulo => ((icono / 10).max(4), (icono / 10).max(4)),
        _ => (if grupo { (icono / 10).max(3) } else { (icono / 5).max(6) }, (icono / 20).max(2)),
    };
    let largos: Vec<i32> = puntos.iter().map(|&a| if a { base * 3 } else { base }).collect();
    let total: i32 = largos.iter().sum::<i32>() + ESPACIO_XXS * (largos.len() as i32 - 1);
    let vertical = aj.posicion.vertical();
    // El hueco del indicador: el relleno, del lado del borde (al final: tras la celda; al principio: antes).
    let (franja_ini, franja) = if aj.posicion.al_final() {
        (if vertical { celda.x + celda.w } else { celda.y + celda.h }, aj.relleno)
    } else {
        (if vertical { celda.x } else { celda.y } - aj.relleno, aj.relleno)
    };
    let traves = franja_ini + (franja - grueso) / 2;
    let mut a = if vertical { celda.y + (celda.h - total) / 2 } else { celda.x + (celda.w - total) / 2 };
    let mut r = Vec::new();
    for (k, &activo) in puntos.iter().enumerate() {
        let l = largos[k];
        r.push((if vertical { Caja { x: traves, y: a, w: grueso, h: l } } else { Caja { x: a, y: traves, w: l, h: grueso } }, activo));
        a += l + ESPACIO_XXS;
    }
    r
}

/// La letra que sale cuando no hay icono (`DockAppButton`: la primera del nombre, en mayúscula; "?" sin nombre).
pub fn inicial(programa: &str) -> String {
    let nombre = programa.rsplit(':').next().unwrap_or(programa);
    match nombre.chars().next() {
        Some(c) => c.to_uppercase().collect(),
        None => "?".to_string(),
    }
}

/// El nombre que se enseña de un programa: el de `Programas/` tal cual; el de una caja, sin el prefijo.
pub fn nombre_visible(programa: &str) -> &str {
    programa.rsplit(':').next().filter(|s| !s.is_empty()).unwrap_or(programa)
}

/// Cuándo se ve el dock (`DockBody.reveal`) y cómo se mueve: el deslizamiento al esconderse y el saltito de cada icono.
#[derive(Clone, Debug)]
pub struct Vista {
    /// `startupRevealDone`: no se enseña hasta 200 ms tras arrancar.
    pub arranque_us: u64,
    /// El ratón encima (`dockMouseArea.containsMouse`), un menú abierto (`contextMenuOpen`, `interactionActive`) y el
    /// rato de gracia tras salir el ratón (`revealSticky` + `revealHold`).
    pub raton: bool,
    pub menu: bool,
    pub gracia_hasta: Option<u64>,
    /// Una ventana a pantalla completa en su monitor (`hiddenForFullscreen`).
    pub pantalla_completa: bool,
    /// Lo tapa una ventana (para `ocultar_con_ventanas`).
    pub tapado: bool,
    /// El deslizamiento (0 en su sitio; `escondido`, fuera), en el muelle "fast" de la duración corta.
    pub desliz: Muelle,
    /// El saltito de cada elemento, por su clave: el tramo y si va subiendo (`bounce`) o bajando (`exit`).
    pub saltos: Vec<(String, Salto)>,
}

#[derive(Clone, Copy, Debug)]
pub struct Salto {
    pub tramo: Tramo,
    /// La segunda mitad del rebote (de 0.25 a 0.20 de la distancia) queda por empezar.
    pub falta_segundo: bool,
}

impl Vista {
    pub fn nueva(ahora_us: u64, escondido: i32) -> Vista {
        let mut desliz = Muelle::preset(Preset::Rapido, muelle::corta_ms(muelle::BASE_MS));
        desliz.saltar(escondido as i64 * UNO);
        Vista { arranque_us: ahora_us.saturating_add(ESPERA_ARRANQUE_US), raton: false, menu: false, gracia_hasta: None, pantalla_completa: false, tapado: false, desliz, saltos: Vec::new() }
    }

    /// `reveal`.
    pub fn se_ve(&self, aj: &Dock, ahora_us: u64) -> bool {
        if ahora_us < self.arranque_us || !aj.activo {
            return false;
        }
        if self.pantalla_completa && !aj.pantalla_completa {
            return false;
        }
        let encima = self.raton || self.menu || self.gracia_hasta.is_some_and(|h| ahora_us < h);
        if aj.ocultar_con_ventanas {
            return !self.tapado || encima;
        }
        !aj.autoocultar || encima
    }

    /// El ratón entra o sale del dock.
    pub fn poner_raton(&mut self, dentro: bool, aj: &Dock, ahora_us: u64) {
        if dentro == self.raton {
            return;
        }
        self.raton = dentro;
        if dentro {
            self.gracia_hasta = None;
        } else if aj.autoocultar && !self.menu {
            self.gracia_hasta = Some(ahora_us.saturating_add(ESPERA_ESCONDER_US));
        }
    }

    /// Se cierra un menú: un rato de gracia antes de esconderse (`onContextMenuOpenChanged`).
    pub fn poner_menu(&mut self, abierto: bool, aj: &Dock, ahora_us: u64) {
        if abierto == self.menu {
            return;
        }
        self.menu = abierto;
        if !abierto && aj.autoocultar && !self.raton {
            self.gracia_hasta = Some(ahora_us.saturating_add(ESPERA_ESCONDER_US));
        }
    }

    /// Lleva el deslizamiento hacia donde toca (visto o escondido). Devuelve si se ve.
    pub fn apuntar(&mut self, aj: &Dock, d: &Disposicion, ahora_us: u64) -> bool {
        let ve = self.se_ve(aj, ahora_us);
        self.desliz.ir(if ve { 0 } else { d.escondido as i64 * UNO });
        ve
    }

    /// El ratón pasa a estar sobre `clave` (o sobre nada): el que deja baja (`exit`) y el nuevo sube (`bounce`), con
    /// `distancia` = el tamaño del icono, hacia dentro de la pantalla.
    pub fn encima_de(&mut self, clave: Option<&str>, distancia: i32, ahora_us: u64, animar: bool) {
        let dur = muelle::DUR_CORTA_ANIMS_MS as u64 * 1000;
        for (k, s) in self.saltos.iter_mut() {
            if Some(k.as_str()) != clave && s.tramo.hasta != 0 {
                s.tramo = s.tramo.hacia(ahora_us, 0, if animar { dur } else { 0 }, Curva::EnfatizadaFrena);
                s.falta_segundo = false;
            }
        }
        let Some(c) = clave else { return };
        let alto = distancia as i64 * UNO / 4;
        let tramo = match self.saltos.iter().find(|(k, _)| k == c) {
            Some((_, s)) if s.tramo.hasta == alto || s.falta_segundo => return,
            Some((_, s)) => s.tramo.hacia(ahora_us, alto, if animar { dur } else { 0 }, Curva::EnfatizadaAcelera),
            None => Tramo::quieto(0).hacia(ahora_us, alto, if animar { dur } else { 0 }, Curva::EnfatizadaAcelera),
        };
        let s = Salto { tramo, falta_segundo: animar };
        match self.saltos.iter_mut().find(|(k, _)| k == c) {
            Some(x) => x.1 = s,
            None => self.saltos.push((c.to_string(), s)),
        }
    }

    /// Avanza las animaciones a `ahora_us` (habiendo pasado `us`). Devuelve si queda alguna en marcha.
    pub fn avanzar(&mut self, ahora_us: u64, us: u64, distancia: i32) -> bool {
        let mut sigue = self.desliz.avanzar(us);
        let dur = muelle::DUR_CORTA_ANIMS_MS as u64 * 1000;
        for (_, s) in self.saltos.iter_mut() {
            if s.tramo.acabado(ahora_us) && s.falta_segundo {
                s.falta_segundo = false;
                s.tramo = s.tramo.hacia(ahora_us, distancia as i64 * UNO / 5, dur, Curva::EnfatizadaFrena);
            }
            if !s.tramo.acabado(ahora_us) {
                sigue = true;
            }
        }
        self.saltos.retain(|(_, s)| !(s.tramo.acabado(ahora_us) && s.tramo.hasta == 0));
        sigue || self.gracia_hasta.is_some_and(|h| ahora_us < h)
    }

    /// Lo que ha subido el icono de `clave` (en píxeles, hacia dentro de la pantalla).
    pub fn salto(&self, clave: &str, ahora_us: u64) -> i32 {
        self.saltos.iter().find(|(k, _)| k == clave).map_or(0, |(_, s)| ((s.tramo.valor(ahora_us) + UNO / 2) / UNO) as i32)
    }
}

/// Una entrada del menú del botón derecho (`DockContextMenu`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opcion {
    /// Una ventana de un grupo (su título; al pulsarla, se activa), con su botón de cerrar.
    Ventana { id: u32, titulo: String },
    Raya,
    Fijar,
    Quitar,
    CerrarVentana,
    CerrarTodas,
}

/// Las opciones del menú de un elemento, en el orden de DMS (sin las acciones de escritorio, que DashOS no tiene).
pub fn opciones(e: &Elemento, ventanas: &[Ventana]) -> Vec<Opcion> {
    let mut o = Vec::new();
    let con_ventanas = e.tipo == Tipo::Grupo && !e.ventanas.is_empty();
    if con_ventanas {
        for (id, _) in &e.ventanas {
            let titulo = ventanas.iter().find(|v| v.id == *id).map_or("", |v| v.titulo.as_str());
            o.push(Opcion::Ventana { id: *id, titulo: if titulo.is_empty() { "(Sin nombre)".to_string() } else { titulo.to_string() } });
        }
        o.push(Opcion::Raya);
    }
    if e.fijable() {
        o.push(if e.fijado { Opcion::Quitar } else { Opcion::Fijar });
    }
    let hay_ventana = e.tipo == Tipo::Ventana || con_ventanas;
    if hay_ventana {
        if e.fijable() {
            o.push(Opcion::Raya);
        }
        o.push(if e.tipo == Tipo::Grupo { Opcion::CerrarTodas } else { Opcion::CerrarVentana });
    }
    o
}

/// El texto de una opción (los de DMS, en castellano).
pub fn texto_opcion(o: &Opcion) -> &str {
    match o {
        Opcion::Ventana { titulo, .. } => titulo,
        Opcion::Raya => "",
        Opcion::Fijar => "Fijar en el dock",
        Opcion::Quitar => "Quitar del dock",
        Opcion::CerrarVentana => "Cerrar la ventana",
        Opcion::CerrarTodas => "Cerrar todas las ventanas",
    }
}

/// Medidas del menú (`DockContextMenuBase`): filas de 28, rayas de 1, una de separación, el relleno de 8, de 180 a 400
/// de ancho, y a 10 de los bordes de la pantalla.
pub const FILA_MENU: i32 = 28;
pub const ANCHO_MIN_MENU: i32 = 180;
pub const ANCHO_MAX_MENU: i32 = 400;
pub const MARGEN_MENU: i32 = 10;

/// Lo que mide el menú: su ancho (de lo que mide el texto más largo, `ancho_texto`) y el alto de sus opciones.
pub fn medir_menu(opciones: &[Opcion], ancho_texto: i32) -> (i32, i32, Vec<Caja>) {
    let ancho = (ancho_texto + ESPACIO_S * 4 + 16 + ESPACIO_XS).clamp(ANCHO_MIN_MENU, ANCHO_MAX_MENU);
    let mut y = ESPACIO_S;
    let mut filas = Vec::new();
    for (k, o) in opciones.iter().enumerate() {
        if k > 0 {
            y += 1;
        }
        let h = if *o == Opcion::Raya { 1 } else { FILA_MENU };
        filas.push(Caja { x: ESPACIO_S, y, w: ancho - ESPACIO_S * 2, h });
        y += h;
    }
    (ancho, y + ESPACIO_S, filas)
}

/// Dónde sale el menú en la pantalla (`updatePosition` y el `x`/`y` de `menuContainer`): junto al botón, por dentro
/// del dock, a la mitad de su grueso más 8.
pub fn sitio_menu(aj: &Dock, boton_centro: (i32, i32), tam: (i32, i32), pantalla: (i32, i32)) -> (i32, i32) {
    let extension = aj.iconos / 2 + ESPACIO_S;
    let (bx, by) = boton_centro;
    let (w, h) = tam;
    let (sw, sh) = pantalla;
    let lim_x = |x: i32| x.max(MARGEN_MENU).min((sw - w - MARGEN_MENU).max(MARGEN_MENU));
    let lim_y = |y: i32| y.max(MARGEN_MENU).min((sh - h - MARGEN_MENU).max(MARGEN_MENU));
    match aj.posicion {
        Borde::Abajo => (lim_x(bx - w / 2), lim_y(by - extension - h)),
        Borde::Arriba => (lim_x(bx - w / 2), lim_y(by + extension)),
        Borde::Izquierda => (lim_x(bx + extension), lim_y(by - h / 2)),
        Borde::Derecha => (lim_x(bx - extension - w), lim_y(by - h / 2)),
    }
}

/// La burbuja con el nombre (`showTooltipForHoveredButton` y `DTooltip`): relleno de 12 a los lados y 8 arriba y
/// abajo, a 8 del dock. Dónde va en la pantalla, para un texto de `tam` (ancho, alto del texto).
pub fn sitio_burbuja(aj: &Dock, boton_centro: (i32, i32), cuerpo_grueso: i32, tam_texto: (i32, i32), pantalla: (i32, i32)) -> Caja {
    let (w, h) = (tam_texto.0 + ESPACIO_M * 2, tam_texto.1 + ESPACIO_S * 2);
    let borde = if aj.borde { aj.grosor_borde } else { 0 };
    let desde_borde = aj.margen + borde + cuerpo_grueso + borde + ESPACIO_S;
    let (sw, sh) = pantalla;
    let lim_x = |x: i32| x.max(ESPACIO_S).min((sw - w - ESPACIO_S).max(ESPACIO_S));
    let lim_y = |y: i32| y.max(ESPACIO_S).min((sh - h - ESPACIO_S).max(ESPACIO_S));
    let (x, y) = match aj.posicion {
        Borde::Abajo => (lim_x(boton_centro.0 - w / 2), sh - desde_borde - h),
        Borde::Arriba => (lim_x(boton_centro.0 - w / 2), desde_borde),
        Borde::Izquierda => (desde_borde, lim_y(boton_centro.1 - h / 2)),
        Borde::Derecha => (sw - desde_borde - w, lim_y(boton_centro.1 - h / 2)),
    };
    Caja { x, y, w, h }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use ajustes::Alinear;

    fn v(id: u32, p: &str, activa: bool) -> Ventana {
        Ventana { id, programa: p.to_string(), titulo: alloc::format!("{} {}", p, id), espacio: 1, salida: 0, activa }
    }

    fn tipos(items: &[Elemento]) -> Vec<(Tipo, String)> {
        items.iter().map(|e| (e.tipo, e.programa.clone())).collect()
    }

    #[test]
    fn sin_agrupar_como_dms() {
        let mut aj = Dock::de_serie();
        aj.fijados = alloc::vec!["Shell".into(), "hola".into()];
        let vs = [v(1, "hola", false), v(2, "explorador", true), v(3, "hola", false)];
        let it = construir(&aj, &vs, None);
        // Lanzador, Shell (fijado, cerrado), hola (fijado: se queda con su primera ventana), separador, y las demás.
        assert_eq!(
            tipos(&it),
            alloc::vec![
                (Tipo::Lanzador, "".into()),
                (Tipo::Fijado, "Shell".into()),
                (Tipo::Ventana, "hola".into()),
                (Tipo::Separador, "".into()),
                (Tipo::Ventana, "explorador".into()),
                (Tipo::Ventana, "hola".into()),
            ]
        );
        assert!(it[2].fijado && it[2].con_indicador());
        assert!(!it[1].con_indicador());
        assert!(it[4].activo());
        assert_eq!(it[2].clave, "pinned_hola");
    }

    #[test]
    fn agrupados_como_dms() {
        let mut aj = Dock::de_serie();
        aj.agrupar = true;
        aj.fijados = alloc::vec!["hola".into()];
        let vs = [v(1, "hola", false), v(2, "explorador", false), v(3, "hola", true)];
        let it = construir(&aj, &vs, None);
        assert_eq!(tipos(&it), alloc::vec![(Tipo::Lanzador, "".into()), (Tipo::Grupo, "hola".into()), (Tipo::Separador, "".into()), (Tipo::Grupo, "explorador".into())]);
        assert_eq!(it[1].ventanas, alloc::vec![(1, false), (3, true)]);
        assert_eq!(it[1].puntos(), alloc::vec![false, true]);
        // Separar fijados y abiertos: el fijado sale aparte, sin sus ventanas, y el grupo con ellas tras la raya.
        aj.separar_fijados = true;
        let it = construir(&aj, &vs, None);
        assert_eq!(it.iter().filter(|e| e.programa == "hola").count(), 2);
    }

    #[test]
    fn desbordado() {
        let mut aj = Dock::de_serie();
        aj.max_abiertos = 2;
        let vs: Vec<Ventana> = (0..5).map(|i| v(i, "p", false)).collect();
        let it = construir(&aj, &vs, None);
        let b = it.iter().find(|e| e.tipo == Tipo::Desbordado).expect("botón");
        assert_eq!(b.cuantos, 3);
        assert_eq!(it.iter().filter(|e| e.en_desbordado).count(), 3);
        // Cerrado, los del desbordado no ocupan; desplegado, sí.
        let d = disponer(&aj, &it, false, (1920, 1080));
        assert_eq!(d.celdas.iter().filter(|c| c.w == 0).count(), 3);
        let d2 = disponer(&aj, &it, true, (1920, 1080));
        assert!(d2.cuerpo.w > d.cuerpo.w);
    }

    #[test]
    fn medidas_de_shiva() {
        // Abajo, iconos de 40, relleno 4, separación 4, margen 0, el lanzador y tres ventanas.
        let aj = Dock::de_serie();
        let vs = [v(1, "a", false), v(2, "b", true), v(3, "c", false)];
        let it = construir(&aj, &vs, None);
        let d = disponer(&aj, &it, false, (1920, 1080));
        // 4 elementos de 40 con 3 huecos de 4 = 172, más el relleno de los lados: 180. La capa, 8 más.
        assert_eq!(d.cuerpo.w, 180);
        assert_eq!(d.cuerpo.h, 48);
        assert_eq!(d.capa.ancho, 188);
        // De través: 48 del cuerpo, 12 (40 × 0,3) y 14 de cabecera (40 × 0,35).
        assert_eq!(d.capa.alto, 74);
        assert_eq!(d.cuerpo.y, 74 - 48);
        assert_eq!(d.capa.anclas, 2);
        assert_eq!(d.capa.zona, -1, "con autoocultar no reserva");
        assert_eq!(d.escondido, 58);
        assert_eq!(d.celdas[0], Caja { x: 8, y: 30, w: 40, h: 40 });
        assert_eq!(d.celdas[1].x, 52);
        assert_eq!(d.entrada_escondida, Caja { x: 0, y: 73, w: 188, h: 1 });
        // El indicador, en los 4 de relleno de abajo: 4 × 4, el activo de 12.
        let p = indicador(&aj, &it[2], d.celdas[2]);
        assert_eq!(p, alloc::vec![(Caja { x: d.celdas[2].x + 14, y: 70, w: 12, h: 4 }, true)]);
        assert_eq!(indicador(&aj, &it[1], d.celdas[1])[0].0.w, 4);
        assert_eq!(d.en(10, 40), Some(0));
        assert_eq!(d.en(0, 0), None);
    }

    #[test]
    fn otros_bordes_y_modo_barra() {
        let mut aj = Dock::de_serie();
        aj.autoocultar = false;
        let vs = [v(1, "a", false)];
        for b in [Borde::Arriba, Borde::Izquierda, Borde::Derecha] {
            aj.posicion = b;
            let it = construir(&aj, &vs, None);
            let d = disponer(&aj, &it, false, (800, 600));
            assert_eq!(d.capa.zona, 48);
            for c in &d.celdas {
                assert!(c.x >= 0 && c.y >= 0 && c.x + c.w <= d.capa.ancho && c.y + c.h <= d.capa.alto, "{b:?}: {c:?} fuera de {:?}", d.capa);
            }
            for (p, _) in indicador(&aj, &it[1], d.celdas[1]) {
                assert!(p.x >= 0 && p.y >= 0 && p.x + p.w <= d.capa.ancho && p.y + p.h <= d.capa.alto, "{b:?}: indicador fuera");
            }
        }
        aj.posicion = Borde::Abajo;
        aj.modo = Modo::Barra;
        aj.alinear = Alinear::Inicio;
        let it = construir(&aj, &vs, None);
        let d = disponer(&aj, &it, false, (800, 600));
        assert_eq!(d.capa.ancho, 800);
        assert_eq!(d.capa.anclas, 2 | 4 | 8);
        assert_eq!(d.celdas[0].x, 4);
    }

    #[test]
    fn hostil_mil_ventanas() {
        let aj = Dock::de_serie();
        let vs: Vec<Ventana> = (0..1000).map(|i| v(i, &alloc::format!("p{}", i % 37), i == 999)).collect();
        let it = construir(&aj, &vs, None);
        assert!(it.len() <= MAX_VENTANAS + 3);
        let d = disponer(&aj, &it, false, (1920, 1080));
        // No pasa de la pantalla.
        assert!(d.capa.ancho <= 1920 && d.cuerpo.w <= 1920);
        // Agrupado: 37 programas.
        let mut aj2 = aj.clone();
        aj2.agrupar = true;
        assert_eq!(construir(&aj2, &vs, None).len(), 1 + 37);
        // Un monitor diminuto o de cero.
        for p in [(0, 0), (1, 1), (-5, 10)] {
            let d = disponer(&aj, &it, false, p);
            assert!(d.capa.ancho >= 1 && d.capa.alto >= 1);
        }
    }

    #[test]
    fn hostil_rafaga_de_abrir_y_cerrar() {
        // Un programa que se abre y se cierra a ráfagas: la lista se rehace cada vez sin dejar restos.
        let mut aj = Dock::de_serie();
        aj.fijados = alloc::vec!["r".into()];
        let mut vs: Vec<Ventana> = Vec::new();
        for i in 0..10_000u32 {
            if i % 3 == 2 {
                vs.clear();
            } else {
                vs.push(v(i, "r", false));
            }
            let it = construir(&aj, &vs, None);
            let fijados = it.iter().filter(|e| e.programa == "r" && e.fijado).count();
            assert_eq!(fijados, 1, "el fijado sale una vez, abierto o no");
            assert_eq!(it.iter().filter(|e| e.tipo == Tipo::Separador).count(), usize::from(vs.len() > 1));
        }
    }

    #[test]
    fn se_ve_y_se_esconde() {
        let aj = Dock::de_serie();
        let it = construir(&aj, &[], None);
        let d = disponer(&aj, &it, false, (1920, 1080));
        let mut vista = Vista::nueva(0, d.escondido);
        assert!(!vista.se_ve(&aj, 100_000), "no antes de 200 ms");
        assert!(!vista.se_ve(&aj, 300_000), "autoocultar: escondido sin el ratón");
        vista.poner_raton(true, &aj, 300_000);
        assert!(vista.apuntar(&aj, &d, 300_000));
        let mut t = 300_000;
        while vista.avanzar(t, 16_667, aj.iconos) {
            t += 16_667;
            assert!(t < 2_000_000);
        }
        assert_eq!(vista.desliz.entero(), 0);
        vista.poner_raton(false, &aj, t);
        assert!(vista.se_ve(&aj, t + 100_000), "250 ms de gracia");
        assert!(!vista.se_ve(&aj, t + 300_000));
        // Sin autoocultar, siempre.
        let mut aj2 = aj.clone();
        aj2.autoocultar = false;
        assert!(vista.se_ve(&aj2, t + 300_000));
        // A pantalla completa, no.
        vista.pantalla_completa = true;
        assert!(!vista.se_ve(&aj2, t + 300_000));
    }

    #[test]
    fn el_saltito() {
        let mut vista = Vista::nueva(0, 58);
        vista.encima_de(Some("a"), 40, 0, true);
        let mut t = 0;
        let mut maximo = 0;
        while vista.avanzar(t, 16_667, 40) && t < 1_000_000 {
            t += 16_667;
            maximo = maximo.max(vista.salto("a", t));
        }
        assert_eq!(maximo, 10, "un cuarto del icono");
        assert_eq!(vista.salto("a", t), 8, "y se queda en un quinto");
        vista.encima_de(None, 40, t, true);
        while vista.avanzar(t, 16_667, 40) && t < 2_000_000 {
            t += 16_667;
        }
        assert_eq!(vista.salto("a", t), 0);
        assert!(vista.saltos.is_empty());
    }

    #[test]
    fn menu() {
        let mut aj = Dock::de_serie();
        aj.agrupar = true;
        let vs = [v(1, "a", false), v(2, "a", false)];
        let it = construir(&aj, &vs, None);
        let o = opciones(&it[1], &vs);
        assert_eq!(o.len(), 6);
        assert!(matches!(o[0], Opcion::Ventana { id: 1, .. }));
        assert_eq!(o[2], Opcion::Raya);
        assert_eq!(o[3], Opcion::Fijar);
        assert_eq!(o[5], Opcion::CerrarTodas);
        assert!(opciones(&it[0], &vs).is_empty(), "el lanzador no tiene menú");
        let (w, h, filas) = medir_menu(&o, 100);
        assert_eq!(w, ANCHO_MIN_MENU);
        assert_eq!(h, 8 + 28 * 4 + 2 + 5 + 8);
        assert_eq!(filas.len(), 6);
        // Siempre dentro de la pantalla.
        for b in [Borde::Abajo, Borde::Arriba, Borde::Izquierda, Borde::Derecha] {
            aj.posicion = b;
            for c in [(0, 0), (1919, 1079), (960, 540), (-100, 5000)] {
                let (x, y) = sitio_menu(&aj, c, (w, h), (1920, 1080));
                assert!(x >= 10 && y >= 10 && x + w <= 1910 && y + h <= 1070, "{b:?} {c:?}: {x},{y}");
            }
        }
    }

    #[test]
    fn inicial_y_nombre() {
        assert_eq!(inicial("hola"), "H");
        assert_eq!(inicial("linux:foot"), "F");
        assert_eq!(inicial(""), "?");
        assert_eq!(inicial("ñu"), "Ñ");
        assert_eq!(nombre_visible("amiga:Workbench"), "Workbench");
        assert_eq!(nombre_visible("linux:"), "linux:");
    }
}
