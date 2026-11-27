//! S27: el centro de control configurable (el panel rápido), como `controlCenterWidgets` de DMS: qué mosaicos y
//! deslizadores salen, en qué orden, de qué ancho y alto (en columnas de una rejilla), y dónde va cada uno.
//!
//! Portado de DMS (DankMaterialShell v2.1.2: `Common/GridLayout.js`, `Modules/ControlCenter/CcMetrics.qml`,
//! `Modules/ControlCenter/utils/widgets.js` y `Common/settings/SettingsSpec.js`; MIT, copyright (c) 2025 Avenge Media
//! LLC; aviso entero en `docs/licencias/DankMaterialShell-MIT.txt`).
//!
//! En texto (`ENV:Escritorio/centro-control`, S36 regla 1):
//!
//! ```text
//! columnas = 8
//! mosaicos = usuario 5, ajustes 1 pequeño, bloquear 1 pequeño, apagar 1 pequeño, volumen 4, brillo 4, red 4, ...
//! ```
//!
//! Cada mosaico: su nombre, su ancho en columnas (de media en media: `4.5`), su alto si no es 1 (`4x2`), `pequeño` (una
//! fila de 48 en vez de 64, como `small`) y su sitio si se fijó (`@columna:fila`). También se aceptan los nombres de DMS
//! (`volumeSlider`, `wifi`...), para traer sus ajustes. Lo de serie es lo del escritorio del usuario en shiva.
//!
//! La rejilla: los mosaicos, en orden, van al primer hueco libre (de izquierda a derecha y de arriba abajo) o a su
//! sitio, y bajan si chocan; al final suben lo que puedan ("gravedad"). Una fila con solo mosaicos pequeños mide 48.
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/cc_rejilla.rs`.

/// Qué es cada mosaico.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    /// Quien está delante (avatar, nombre, tiempo encendido).
    Usuario,
    /// Un botón de icono (ajustes, bloquear, apagar, editar).
    Accion,
    /// Un deslizador (volumen, brillo).
    Deslizador,
    /// Un mosaico con su icono y dos líneas (red, modo oscuro...).
    Mosaico,
}

/// Un mosaico del catálogo: su nombre, el de DMS, el que se ve, su tipo, su ancho de serie y si DashOS tiene con qué
/// llevarlo (si no, sale apagado: "no disponible").
#[derive(Clone, Copy, Debug)]
pub struct Def {
    pub id: &'static str,
    pub dms: &'static str,
    pub nombre: &'static str,
    pub tipo: Tipo,
    pub ancho: u8,
    pub disponible: bool,
}

const fn d(id: &'static str, dms: &'static str, nombre: &'static str, tipo: Tipo, ancho: u8, disponible: bool) -> Def {
    Def { id, dms, nombre, tipo, ancho, disponible }
}

/// El catálogo (`Models/WidgetModel.qml` de DMS), con los que DashOS ya lleva y los que aún no.
pub const CATALOGO: &[Def] = &[
    d("usuario", "user", "Usuario", Tipo::Usuario, 5, true),
    d("ajustes", "settings", "Ajustes", Tipo::Accion, 1, true),
    d("bloquear", "lock", "Bloquear", Tipo::Accion, 1, true),
    d("apagar", "power", "Apagar", Tipo::Accion, 1, true),
    d("editar", "edit", "Editar", Tipo::Accion, 1, true),
    d("volumen", "volumeSlider", "Volumen", Tipo::Deslizador, 4, true),
    d("brillo", "brightnessSlider", "Brillo", Tipo::Deslizador, 4, false),
    d("red", "wifi", "Red", Tipo::Mosaico, 4, true),
    d("bluetooth", "bluetooth", "Bluetooth", Tipo::Mosaico, 4, false),
    d("salida", "audioOutput", "Salida de sonido", Tipo::Mosaico, 4, true),
    d("entrada", "audioInput", "Micrófono", Tipo::Mosaico, 4, false),
    d("noche", "nightMode", "Modo noche", Tipo::Mosaico, 4, false),
    d("oscuro", "darkMode", "Modo oscuro", Tipo::Mosaico, 4, true),
    d("no_molestar", "doNotDisturb", "No molestar", Tipo::Mosaico, 4, true),
    d("despierto", "idleInhibitor", "Mantener despierto", Tipo::Mosaico, 4, false),
    d("bateria", "battery", "Batería", Tipo::Mosaico, 4, false),
    d("disco", "diskUsage", "Disco", Tipo::Mosaico, 4, false),
    d("programas", "runningApps", "Programas abiertos", Tipo::Mosaico, 4, false),
    // Rama `fondos`: abre la herramienta de fondos (en DMS, el fondo va en el Dash; aquí, también en el centro de control).
    d("fondos", "wallpaper", "Fondos", Tipo::Accion, 1, true),
];

/// El índice en el catálogo de un nombre (el de DashOS o el de DMS, sin distinguir mayúsculas).
pub fn buscar(nombre: &str) -> Option<usize> {
    CATALOGO.iter().position(|x| x.id.eq_ignore_ascii_case(nombre) || x.dms.eq_ignore_ascii_case(nombre))
}

/// Cuántos mosaicos caben en el texto; y las columnas (`controlCenterColumns`: de 4 a 12, 8 de serie).
pub const MAX: usize = 24;
pub const MIN_COLUMNAS: u8 = 4;
pub const MAX_COLUMNAS: u8 = 12;

/// Un mosaico puesto: del catálogo, ancho y alto en medias columnas, pequeño y su sitio (en medias), si se fijó.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Puesto {
    pub def: u8,
    pub w2: u8,
    pub h2: u8,
    pub pequeno: bool,
    pub sitio: Option<(u8, u8)>,
}

impl Puesto {
    pub fn def(&self) -> &'static Def {
        &CATALOGO[self.def as usize]
    }

    /// Lo de serie de un mosaico del catálogo (al añadirlo: los botones y los deslizadores, pequeños, como DMS).
    pub fn nuevo(def: usize) -> Puesto {
        let x = &CATALOGO[def];
        Puesto { def: def as u8, w2: 2 * x.ancho, h2: 2, pequeno: matches!(x.tipo, Tipo::Accion), sitio: None }
    }
}

/// Lo que hay en el centro de control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rejilla {
    pub columnas: u8,
    pub n: usize,
    pub puestos: [Puesto; MAX],
}

/// Por qué no vale un texto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mal {
    Columnas,
    Demasiados,
    Mosaico,
    Medida,
    Repetido,
}

fn medias(t: &str) -> Option<u8> {
    let (e, f) = t.split_once('.').unwrap_or((t, ""));
    let e: u8 = e.parse().ok()?;
    let m = match f {
        "" | "0" => 0,
        "5" => 1,
        _ => return None,
    };
    (e as u16 * 2 + m as u16).try_into().ok().filter(|&x: &u8| x >= 1)
}

impl Rejilla {
    /// Lo del escritorio de shiva (su `controlCenterWidgets`).
    pub fn de_serie() -> Rejilla {
        let mut r = Rejilla { columnas: 8, n: 0, puestos: [Puesto { def: 0, w2: 2, h2: 2, pequeno: false, sitio: None }; MAX] };
        for (id, w, pequeno) in [("usuario", 5, false), ("ajustes", 1, true), ("bloquear", 1, true), ("apagar", 1, true), ("volumen", 4, false), ("brillo", 4, false),
            ("red", 4, false), ("bluetooth", 4, false), ("salida", 4, false), ("entrada", 4, false), ("noche", 4, false), ("oscuro", 4, false), ("editar", 1, true)]
        {
            r.puestos[r.n] = Puesto { def: buscar(id).unwrap() as u8, w2: 2 * w, h2: 2, pequeno, sitio: None };
            r.n += 1;
        }
        r.puestos[r.n] = Puesto { def: buscar("programas").unwrap() as u8, w2: 8, h2: 2, pequeno: true, sitio: Some((0, 12)) };
        r.n += 1;
        r
    }

    pub fn puestos(&self) -> &[Puesto] {
        &self.puestos[..self.n]
    }

    /// Lee `columnas` y `mosaicos` (sus valores, sin la clave). Lo que no se entiende, error: quien lee se queda con lo de
    /// serie. "editar" se pone siempre (como DMS: sin él no se podría volver a editar).
    pub fn leer(columnas: Option<&str>, mosaicos: Option<&str>) -> Result<Rejilla, Mal> {
        let mut r = Rejilla::de_serie();
        if let Some(c) = columnas {
            r.columnas = c.trim().parse().ok().filter(|c| (MIN_COLUMNAS..=MAX_COLUMNAS).contains(c)).ok_or(Mal::Columnas)?;
        }
        let Some(m) = mosaicos else { return Ok(r) };
        r.n = 0;
        for trozo in m.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            if r.n == MAX {
                return Err(Mal::Demasiados);
            }
            let mut partes = trozo.split_whitespace();
            let def = buscar(partes.next().ok_or(Mal::Mosaico)?).ok_or(Mal::Mosaico)?;
            if r.puestos().iter().any(|p| p.def as usize == def) {
                return Err(Mal::Repetido);
            }
            let mut p = Puesto::nuevo(def);
            p.pequeno = false;
            for x in partes {
                if x == "pequeño" || x == "pequeno" || x.eq_ignore_ascii_case("small") {
                    p.pequeno = true;
                } else if let Some(s) = x.strip_prefix('@') {
                    let (c, f) = s.split_once(':').ok_or(Mal::Medida)?;
                    p.sitio = Some((medias(c).or((c == "0").then_some(0)).ok_or(Mal::Medida)?, medias(f).or((f == "0").then_some(0)).ok_or(Mal::Medida)?));
                } else {
                    let (w, h) = x.split_once('x').unwrap_or((x, "1"));
                    p.w2 = medias(w).ok_or(Mal::Medida)?;
                    p.h2 = medias(h).ok_or(Mal::Medida)?;
                    if p.w2 > 2 * MAX_COLUMNAS || p.h2 > 16 {
                        return Err(Mal::Medida);
                    }
                }
            }
            r.puestos[r.n] = p;
            r.n += 1;
        }
        let editar = buscar("editar").unwrap();
        if !r.puestos().iter().any(|p| p.def as usize == editar) && r.n < MAX {
            r.puestos[r.n] = Puesto::nuevo(editar);
            r.n += 1;
        }
        Ok(r)
    }

    /// El valor de `mosaicos` en texto, en `sal`. Devuelve cuántos bytes (o `None` si no cabe).
    pub fn escribir(&self, sal: &mut [u8]) -> Option<usize> {
        let mut n = 0;
        let mut pon = |s: &str, sal: &mut [u8]| -> Option<()> {
            sal.get_mut(n..n + s.len())?.copy_from_slice(s.as_bytes());
            n += s.len();
            Some(())
        };
        let num = |m: u8| -> ([u8; 6], usize) {
            let mut b = [0u8; 6];
            let e = m / 2;
            let mut i = 0;
            if e >= 10 {
                b[i] = b'0' + e / 10;
                i += 1;
            }
            b[i] = b'0' + e % 10;
            i += 1;
            if m % 2 == 1 {
                b[i] = b'.';
                b[i + 1] = b'5';
                i += 2;
            }
            (b, i)
        };
        for (k, p) in self.puestos().iter().enumerate() {
            if k > 0 {
                pon(", ", sal)?;
            }
            pon(p.def().id, sal)?;
            pon(" ", sal)?;
            let (b, l) = num(p.w2);
            pon(core::str::from_utf8(&b[..l]).unwrap(), sal)?;
            if p.h2 != 2 {
                pon("x", sal)?;
                let (b, l) = num(p.h2);
                pon(core::str::from_utf8(&b[..l]).unwrap(), sal)?;
            }
            if p.pequeno {
                pon(" pequeño", sal)?;
            }
            if let Some((c, f)) = p.sitio {
                pon(" @", sal)?;
                let (b, l) = num(c);
                pon(if c == 0 { "0" } else { core::str::from_utf8(&b[..l]).unwrap() }, sal)?;
                pon(":", sal)?;
                let (b, l) = num(f);
                pon(if f == 0 { "0" } else { core::str::from_utf8(&b[..l]).unwrap() }, sal)?;
            }
        }
        Some(n)
    }

    /// En bytes, para el canal (el centro se la da al panel; el panel se la pide guardar): versión, columnas, cuántos y,
    /// de cada uno, mosaico, ancho, alto, banderas (1 pequeño, 2 con sitio), columna y fila. Devuelve cuántos.
    pub fn escribir_bytes(&self, m: &mut [u8]) -> usize {
        m[0] = 1;
        m[1] = self.columnas;
        m[2] = self.n as u8;
        for (k, p) in self.puestos().iter().enumerate() {
            let (c, f) = p.sitio.unwrap_or((0, 0));
            m[3 + 6 * k..9 + 6 * k].copy_from_slice(&[p.def, p.w2, p.h2, p.pequeno as u8 | (p.sitio.is_some() as u8) << 1, c, f]);
        }
        3 + 6 * self.n
    }

    /// De bytes, desconfiando: lo que no cuadra, `None`.
    pub fn leer_bytes(m: &[u8]) -> Option<Rejilla> {
        let (&v, &c, &n) = (m.first()?, m.get(1)?, m.get(2)?);
        let n = n as usize;
        if v != 1 || !(MIN_COLUMNAS..=MAX_COLUMNAS).contains(&c) || n > MAX || m.len() != 3 + 6 * n {
            return None;
        }
        let mut r = Rejilla::de_serie();
        r.columnas = c;
        r.n = 0;
        for k in 0..n {
            let x = &m[3 + 6 * k..9 + 6 * k];
            if x[0] as usize >= CATALOGO.len() || x[1] == 0 || x[1] > 2 * MAX_COLUMNAS || x[2] == 0 || x[2] > 16 || x[3] > 3 || r.puestos().iter().any(|p| p.def == x[0]) {
                return None;
            }
            r.puestos[k] = Puesto { def: x[0], w2: x[1], h2: x[2], pequeno: x[3] & 1 != 0, sitio: (x[3] & 2 != 0).then_some((x[4], x[5].min(64))) };
            r.n += 1;
        }
        Some(r)
    }

    /// Quita el del puesto `k` ("editar" no se quita).
    pub fn quitar(&mut self, k: usize) -> bool {
        if k >= self.n || self.puestos[k].def().id == "editar" {
            return false;
        }
        self.puestos.copy_within(k + 1..self.n, k);
        self.n -= 1;
        true
    }

    /// Añade uno del catálogo al final, con lo de serie (si no está ya y cabe).
    pub fn anadir(&mut self, def: usize) -> bool {
        if def >= CATALOGO.len() || self.n == MAX || self.puestos().iter().any(|p| p.def as usize == def) {
            return false;
        }
        self.puestos[self.n] = Puesto::nuevo(def);
        self.n += 1;
        true
    }

    /// Mueve el del puesto `k` a `a` (y se olvida su sitio fijo: va donde le toque por orden).
    pub fn mover(&mut self, k: usize, a: usize) -> bool {
        if k >= self.n || a >= self.n || k == a {
            return false;
        }
        let mut p = self.puestos[k];
        p.sitio = None;
        if k < a {
            self.puestos.copy_within(k + 1..=a, k);
        } else {
            self.puestos.copy_within(a..k, a + 1);
        }
        self.puestos[a] = p;
        true
    }

    /// Cambia el ancho (en medias columnas) del puesto `k`, dentro de lo que se deja (los deslizadores, 2 columnas como
    /// poco; nada más ancho que la rejilla).
    pub fn ancho(&mut self, k: usize, w2: u8) -> bool {
        let Some(p) = self.puestos.get_mut(k).filter(|_| k < self.n) else { return false };
        let minimo = if p.def().tipo == Tipo::Deslizador { 4 } else { 1 };
        let w2 = w2.clamp(minimo, 2 * self.columnas);
        let cambia = w2 != p.w2;
        p.w2 = w2;
        cambia
    }
}

/// Medidas de DMS (`CcMetrics`): relleno 16, columna y fila de 64, fila pequeña de 48, 8 entre una y otra.
pub const RELLENO: i32 = 16;
pub const COLUMNA: i32 = 64;
pub const FILA: i32 = 64;
pub const FILA_PEQUENA: i32 = 48;
pub const HUECO: i32 = 8;

/// El ancho del panel con `c` columnas.
pub const fn ancho_panel(c: u8) -> i32 {
    2 * RELLENO + c as i32 * COLUMNA + (c as i32 - 1) * HUECO
}

/// Un rectángulo (x, y, ancho, alto), relativo a la rejilla.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Caja {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// La rejilla colocada: la caja de cada puesto (en el orden de los puestos) y lo que mide.
#[derive(Clone, Copy, Debug)]
pub struct Colocada {
    pub cajas: [Caja; MAX],
    /// Si cada puesto queda en una fila pequeña (su cuerpo, de 48).
    pub encogido: [bool; MAX],
    pub n: usize,
    pub alto: i32,
}

#[derive(Clone, Copy, Default)]
struct Celda {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

fn choca(ocupado: &[Celda], c: &Celda) -> bool {
    ocupado.iter().any(|o| c.x < o.x + o.w && o.x < c.x + c.w && c.y < o.y + o.h && o.y < c.y + c.h)
}

/// Coloca los puestos como `packCells` de DMS (en medias columnas, con gravedad) y luego encoge las filas pequeñas
/// (`shrinkRows`). `extra`: un hueco a todo lo ancho bajo el puesto `.0` de `.1` de alto (el detalle de un mosaico,
/// `insertBand`); lo de debajo baja.
pub fn colocar(r: &Rejilla, extra: Option<(usize, i32)>) -> Colocada {
    let pasos = 2 * r.columnas as i32;
    let mut celdas = [Celda::default(); MAX];
    let mut ocupado: [Celda; MAX] = [Celda::default(); MAX];
    let mut n = 0;
    for (k, p) in r.puestos().iter().enumerate() {
        let w = (p.w2 as i32).clamp(1, pasos);
        let h = (p.h2 as i32).max(1);
        let mut c = match p.sitio {
            Some((col, fila)) => Celda { x: (col as i32).min(pasos - w).max(0), y: fila as i32, w, h },
            None => {
                // El primer hueco, por filas, hasta lo más alto ocupado; si no, debajo de todo.
                let techo = ocupado[..n].iter().map(|o| o.y + o.h).max().unwrap_or(0);
                let mut sitio = Celda { x: 0, y: techo, w, h };
                'busca: for y in 0..techo {
                    for x in 0..=pasos - w {
                        let prueba = Celda { x, y, w, h };
                        if !choca(&ocupado[..n], &prueba) {
                            sitio = prueba;
                            break 'busca;
                        }
                    }
                }
                sitio
            }
        };
        while choca(&ocupado[..n], &c) {
            c.y += 1;
        }
        ocupado[n] = c;
        n += 1;
        celdas[k] = c;
    }
    // Gravedad: de arriba abajo, cada uno sube mientras no choque.
    let mut orden: [usize; MAX] = core::array::from_fn(|i| i);
    orden[..n].sort_by_key(|&i| (celdas[i].y, celdas[i].x));
    for &i in &orden[..n] {
        while celdas[i].y > 0 {
            let mut prueba = celdas[i];
            prueba.y -= 1;
            let otros = (0..n).filter(|&j| j != i).any(|j| {
                let o = celdas[j];
                prueba.x < o.x + o.w && o.x < prueba.x + prueba.w && prueba.y < o.y + o.h && o.y < prueba.y + prueba.h
            });
            if otros {
                break;
            }
            celdas[i] = prueba;
        }
    }
    // Las filas, de media en media (`shrinkRows`): cada media fila mide la mitad de la fila menos medio hueco (28, y 20
    // si solo la ocupan pequeños: 48 la fila entera), con un hueco entero entre medias.
    let bandas = celdas[..n].iter().map(|c| c.y + c.h).max().unwrap_or(0) as usize;
    let mut alto_banda = [0i32; 4 * MAX];
    let mut tops = [0i32; 4 * MAX + 1];
    let completa = FILA / 2 - HUECO / 2;
    let corta = FILA_PEQUENA / 2 - HUECO / 2;
    let mut encoge = [false; MAX];
    for (k, e) in encoge.iter_mut().enumerate().take(n) {
        *e = r.puestos[k].pequeno && r.puestos[k].h2 <= 2;
    }
    for b in 0..bandas.min(4 * MAX) {
        let ocupantes = (0..n).filter(|&k| celdas[k].y as usize <= b && b < (celdas[k].y + celdas[k].h) as usize);
        let mut hay = false;
        let mut todos = true;
        for k in ocupantes {
            hay = true;
            todos &= encoge[k];
        }
        alto_banda[b] = if hay && todos { corta } else { completa };
    }
    let mut y = 0;
    for b in 0..bandas.min(4 * MAX) {
        tops[b] = y;
        y += alto_banda[b] + HUECO;
    }
    let mut alto = (y - HUECO).max(0);
    // Media columna: 36 (una columna y su hueco, 72, partida); un ancho de w medias, w × 36 menos un hueco.
    let colw = (COLUMNA + HUECO) / 2;
    let mut cajas = [Caja::default(); MAX];
    let mut encogido = [false; MAX];
    for k in 0..n {
        let c = celdas[k];
        let (b0, b1) = (c.y as usize, (c.y + c.h) as usize);
        let h: i32 = alto_banda[b0..b1].iter().sum::<i32>() + (c.h - 1) * HUECO;
        encogido[k] = alto_banda[b0..b1].iter().all(|&a| a == corta);
        let x = c.x * colw;
        let w = c.w * colw - HUECO;
        cajas[k] = Caja { x, y: tops[b0], w, h };
    }
    // El hueco del detalle: bajo el puesto (y bajo lo que cruce su borde de abajo), lo de debajo baja.
    if let Some((k, extension)) = extra.filter(|e| e.0 < n && e.1 > 0) {
        let mut corte = cajas[k].y + cajas[k].h;
        loop {
            let mas = (0..n).filter(|&j| cajas[j].y < corte && cajas[j].y + cajas[j].h > corte).map(|j| cajas[j].y + cajas[j].h).max();
            match mas {
                Some(m) => corte = m,
                None => break,
            }
        }
        for c in cajas.iter_mut().take(n).filter(|c| c.y >= corte) {
            c.y += extension;
        }
        alto += extension;
    }
    Colocada { cajas, encogido, n, alto }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn texto(r: &Rejilla) -> String {
        let mut b = [0u8; 1024];
        let n = r.escribir(&mut b).unwrap();
        String::from_utf8(b[..n].to_vec()).unwrap()
    }

    #[test]
    fn el_de_shiva() {
        let r = Rejilla::de_serie();
        assert_eq!(ancho_panel(r.columnas), 600);
        let c = colocar(&r, None);
        // Fila 0: usuario (5) y los tres botones; fila 1: volumen y brillo; 2: red, bluetooth; 3: salida, entrada; 4: noche,
        // oscuro; 5: editar solo (pequeña); 6: programas (pequeña). 5 filas de 64, 2 de 48 y 6 huecos: 464 sin el relleno
        // (DMS dice 496 con 32 de relleno).
        assert_eq!(c.alto + 2 * RELLENO, 496);
        assert_eq!(c.cajas[0], Caja { x: 0, y: 0, w: 352, h: 64 });
        assert_eq!(c.cajas[1].x, 360);
        assert_eq!(c.cajas[4], Caja { x: 0, y: 72, w: 280, h: 64 });
        assert_eq!(c.cajas[5].x, 288);
        let editar = c.cajas[12];
        let programas = c.cajas[13];
        assert_eq!((editar.y, editar.h), (5 * 72, 48));
        assert_eq!((programas.y, programas.h, programas.w), (5 * 72 + 56, 48, 280));
        assert!(c.encogido[12] && c.encogido[13] && !c.encogido[1]);
    }

    #[test]
    fn texto_ida_y_vuelta() {
        let r = Rejilla::de_serie();
        let t = texto(&r);
        assert!(t.starts_with("usuario 5, ajustes 1 pequeño, bloquear 1 pequeño"));
        assert!(t.ends_with("programas 4 pequeño @0:6"));
        let x = Rejilla::leer(Some("8"), Some(&t)).unwrap();
        assert_eq!((x.columnas, x.puestos()), (r.columnas, r.puestos()));
        // Con los nombres de DMS, medias y altos.
        let x = Rejilla::leer(Some("6"), Some("volumeSlider 4.5, wifi 2x2, darkMode 1.5 small")).unwrap();
        assert_eq!((x.columnas, x.n), (6, 4));
        assert_eq!((x.puestos[0].w2, x.puestos[1].h2, x.puestos[2].pequeno), (9, 4, true));
        assert_eq!(x.puestos[3].def().id, "editar");
        assert_eq!(texto(&x), "volumen 4.5, red 2x2, oscuro 1.5 pequeño, editar 1 pequeño");
    }

    #[test]
    fn textos_malos() {
        for (c, m, e) in [(Some("3"), None, Mal::Columnas), (Some("13"), None, Mal::Columnas), (Some("ocho"), None, Mal::Columnas), (None, Some("tostadora 4"), Mal::Mosaico),
            (None, Some("red 4, red 2"), Mal::Repetido), (None, Some("red 4.3"), Mal::Medida), (None, Some("red 0"), Mal::Medida), (None, Some("red 99"), Mal::Medida),
            (None, Some("red 4 @x:1"), Mal::Medida), (None, Some("red 4 @1"), Mal::Medida), (None, Some("red -1"), Mal::Medida), (None, Some("red 4x0"), Mal::Medida)]
        {
            assert_eq!(Rejilla::leer(c, m), Err(e), "{:?} {:?}", c, m);
        }
        // Demasiados: más de MAX.
        let mucho: String = (0..30).map(|_| "red 1").collect::<Vec<_>>().join(", ");
        assert!(Rejilla::leer(None, Some(&mucho)).is_err());
        // Vacío: solo "editar".
        let v = Rejilla::leer(None, Some("")).unwrap();
        assert_eq!((v.n, v.puestos[0].def().id), (1, "editar"));
        // Basura de bytes: nunca rompe.
        let mut s = 7u32;
        for _ in 0..2000 {
            let mut b = [0u8; 40];
            for x in b.iter_mut() {
                s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
                *x = b" ,.x@0123456789redvolumnpqe"[(s >> 16) as usize % 27];
            }
            if let Ok(t) = core::str::from_utf8(&b) {
                let _ = Rejilla::leer(Some("8"), Some(t)).map(|r| colocar(&r, Some((0, 100))));
            }
        }
    }

    #[test]
    fn bytes_ida_y_vuelta() {
        let r = Rejilla::de_serie();
        let mut m = [0u8; 3 + 6 * MAX];
        let n = r.escribir_bytes(&mut m);
        let x = Rejilla::leer_bytes(&m[..n]).unwrap();
        assert_eq!((x.columnas, x.puestos()), (r.columnas, r.puestos()));
        assert!(Rejilla::leer_bytes(&m[..n - 1]).is_none());
        let mut y = m;
        y[3] = 200;
        assert!(Rejilla::leer_bytes(&y[..n]).is_none());
        let mut y = m;
        y[9] = y[3];
        assert!(Rejilla::leer_bytes(&y[..n]).is_none());
        let mut y = m;
        y[1] = 2;
        assert!(Rejilla::leer_bytes(&y[..n]).is_none());
        assert!(Rejilla::leer_bytes(&[]).is_none());
    }

    #[test]
    fn editar() {
        let mut r = Rejilla::de_serie();
        let editar = r.n - 2;
        assert!(!r.quitar(editar));
        assert!(r.quitar(0) && r.puestos[0].def().id == "ajustes");
        assert!(!r.anadir(buscar("red").unwrap()));
        assert!(r.anadir(buscar("no_molestar").unwrap()));
        assert_eq!(r.puestos[r.n - 1].w2, 8);
        // Mover olvida el sitio fijo.
        let ultimo = r.n - 2;
        assert!(r.mover(ultimo, 0) && r.puestos[0].sitio.is_none() && r.puestos[0].def().id == "programas");
        assert!(!r.mover(0, 99));
        // Los deslizadores, de 2 columnas como poco; nada más ancho que la rejilla.
        let vol = r.puestos().iter().position(|p| p.def().id == "volumen").unwrap();
        r.ancho(vol, 1);
        assert_eq!(r.puestos[vol].w2, 4);
        r.ancho(vol, 200);
        assert_eq!(r.puestos[vol].w2, 16);
        // Todo lo que se pueda añadir, hasta el tope.
        for k in 0..CATALOGO.len() {
            r.anadir(k);
        }
        assert_eq!(r.n, CATALOGO.len());
        let c = colocar(&r, None);
        // Ninguno se pisa.
        for i in 0..c.n {
            for j in 0..i {
                let (a, b) = (c.cajas[i], c.cajas[j]);
                assert!(!(a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h), "{} y {}", i, j);
            }
        }
    }

    #[test]
    fn el_detalle_empuja() {
        let r = Rejilla::de_serie();
        let sin = colocar(&r, None);
        // El detalle de la red (fila 2): lo de las filas 3 en adelante baja 300; lo de antes no se mueve.
        let con = colocar(&r, Some((6, 300)));
        assert_eq!(con.alto, sin.alto + 300);
        assert_eq!(con.cajas[6], sin.cajas[6]);
        assert_eq!(con.cajas[8].y, sin.cajas[8].y + 300);
        assert_eq!(con.cajas[4].y, sin.cajas[4].y);
    }
}
