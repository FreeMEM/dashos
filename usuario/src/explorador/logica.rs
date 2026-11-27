//! El explorador de archivos, sin dibujo ni servicio: nombres que se pueden enseñar, orden de las entradas, selección,
//! movimiento con el teclado, el camino que se escribe con Ctrl+L, los nombres de las copias, los textos (tamaño,
//! fecha, tipo) y la ficha de lo que va a la papelera.
//!
//! Se prueba en el PC con `rustc --edition 2024 --test usuario/src/explorador/pruebas.rs`.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

/// Un nombre que se puede enseñar y buscar sin dudas (S6b): ni vacío, ni "." ni "..", sin '/', ':' (separan caminos y
/// volúmenes), sin '|', sin letras de control y sin pasar de 200 bytes.
pub fn nombre_valido(n: &str) -> bool {
    !n.is_empty() && n.len() <= 200 && n != "." && n != ".." && !n.contains(['/', ':', '|']) && !n.chars().any(char::is_control)
}

/// Lo que no se enseña sin pedirlo (Ctrl+H): lo que empieza por punto y las copias de seguridad que acaban en '~'.
pub fn es_oculto(n: &str) -> bool {
    n.starts_with('.') || n.ends_with('~')
}

/// Una entrada de un cajón.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entrada {
    pub nombre: String,
    pub cajon: bool,
    pub tam: u64,
    pub fecha: u64,
    pub modo: u16,
    /// Los bits del Amiga (archivado, puro, guion...).
    pub bits: u16,
}

impl Entrada {
    pub fn programa(&self) -> bool {
        !self.cajon && self.modo & 0o111 != 0
    }

    /// La extensión, en minúsculas (sin el punto), si la hay y el nombre no es solo eso.
    pub fn extension(&self) -> Option<String> {
        let (base, ext) = self.nombre.rsplit_once('.')?;
        if base.is_empty() || ext.is_empty() || ext.len() > 8 {
            return None;
        }
        Some(ext.to_lowercase())
    }

    pub fn es_imagen(&self) -> bool {
        !self.cajon && matches!(self.extension().as_deref(), Some("png"))
    }
}

/// Qué clase de cosa es, para el icono y la columna "Tipo".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clase {
    Carpeta,
    Programa,
    Imagen,
    Texto,
    Disco,
    Icono,
    Datos,
    Fichero,
}

pub fn clase_de(e: &Entrada) -> Clase {
    if e.cajon {
        return Clase::Carpeta;
    }
    match e.extension().as_deref() {
        Some("png" | "jpg" | "jpeg" | "gif" | "iff" | "ilbm" | "bmp") => Clase::Imagen,
        Some("txt" | "md" | "doc" | "readme" | "guide" | "cfg" | "conf" | "rs" | "c" | "h" | "dash" | "py" | "sh") => Clase::Texto,
        Some("adf" | "hdf" | "iso" | "img" | "dms") => Clase::Disco,
        Some("info") => Clase::Icono,
        Some("bin" | "dat") => Clase::Datos,
        _ if e.programa() => Clase::Programa,
        _ => Clase::Fichero,
    }
}

/// El tipo, para la ventana (en el idioma del usuario).
pub fn tipo_texto(e: &Entrada) -> String {
    let ext = e.extension().map(|x| x.to_uppercase()).unwrap_or_default();
    String::from(match clase_de(e) {
        Clase::Carpeta => crate::tr(60, "Carpeta"),
        Clase::Programa => crate::tr(100, "Programa"),
        Clase::Imagen if ext.is_empty() => crate::tr(101, "Imagen"),
        Clase::Imagen => return crate::trf(102, "Imagen {}", &[&ext]),
        Clase::Texto => crate::tr(103, "Texto"),
        Clase::Disco => crate::tr(104, "Imagen de disco"),
        Clase::Icono => crate::tr(105, "Icono de Amiga"),
        Clase::Datos => crate::tr(106, "Datos"),
        Clase::Fichero => crate::tr(107, "Fichero"),
    })
}

/// El tipo, para la consola: siempre en castellano.
pub fn tipo_fijo(e: &Entrada) -> String {
    let ext = e.extension().map(|x| x.to_uppercase()).unwrap_or_default();
    String::from(match clase_de(e) {
        Clase::Carpeta => "Carpeta",
        Clase::Programa => "Programa",
        Clase::Imagen if ext.is_empty() => "Imagen",
        Clase::Imagen => return alloc::format!("Imagen {}", ext),
        Clase::Texto => "Texto",
        Clase::Disco => "Imagen de disco",
        Clase::Icono => "Icono de Amiga",
        Clase::Datos => "Datos",
        Clase::Fichero => "Fichero",
    })
}

/// Por qué columna se ordena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orden {
    Nombre,
    Tamano,
    Tipo,
    Fecha,
}

/// Compara dos nombres como una persona: sin distinguir mayúsculas y con los números por su valor ("foto 2" antes que
/// "foto 10").
pub fn comparar_nombres(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut n1 = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    n1.push(c);
                    x.next();
                }
                let mut n2 = String::new();
                while let Some(d) = y.peek().copied().filter(char::is_ascii_digit) {
                    n2.push(d);
                    y.next();
                }
                let (t1, t2) = (n1.trim_start_matches('0'), n2.trim_start_matches('0'));
                let o = t1.len().cmp(&t2.len()).then_with(|| t1.cmp(t2));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Ordena las entradas (sus números en `v`): las carpetas siempre primero, como Nautilus; dentro, por la columna y en
/// el sentido pedidos, y a igualdad por el nombre.
pub fn ordenar(entradas: &[Entrada], v: &mut [usize], orden: Orden, ascendente: bool) {
    v.sort_by(|&i, &j| {
        let (a, b) = (&entradas[i], &entradas[j]);
        let o = match orden {
            Orden::Nombre => comparar_nombres(&a.nombre, &b.nombre),
            Orden::Tamano => a.tam.cmp(&b.tam).then_with(|| comparar_nombres(&a.nombre, &b.nombre)),
            Orden::Tipo => tipo_texto(a).cmp(&tipo_texto(b)).then_with(|| comparar_nombres(&a.nombre, &b.nombre)),
            Orden::Fecha => a.fecha.cmp(&b.fecha).then_with(|| comparar_nombres(&a.nombre, &b.nombre)),
        };
        b.cajon.cmp(&a.cajon).then(if ascendente { o } else { o.reverse() })
    });
}

/// Si `nombre` casa con lo que se busca: sin distinguir mayúsculas ni acentos.
pub fn casa(nombre: &str, busca: &str) -> bool {
    let b = plano(busca.trim());
    b.is_empty() || plano(nombre).contains(&b)
}

/// En minúsculas y sin acentos (las vocales del español y del francés, la ñ se queda).
pub fn plano(t: &str) -> String {
    t.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            c => c,
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Selección: por su sitio en lo que se ve (no en el cajón).

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seleccion {
    pub marcados: Vec<bool>,
    /// Desde dónde se extiende con Mayúsculas.
    pub ancla: Option<usize>,
    /// Lo que tiene el foco del teclado.
    pub cursor: Option<usize>,
}

impl Seleccion {
    pub fn nueva(n: usize) -> Self {
        Seleccion { marcados: alloc::vec![false; n], ancla: None, cursor: None }
    }

    pub fn cuantos(&self) -> usize {
        self.marcados.iter().filter(|m| **m).count()
    }

    pub fn elegidos(&self) -> Vec<usize> {
        self.marcados.iter().enumerate().filter(|x| *x.1).map(|x| x.0).collect()
    }

    pub fn esta(&self, i: usize) -> bool {
        self.marcados.get(i).copied().unwrap_or(false)
    }

    pub fn nada(&mut self) {
        self.marcados.iter_mut().for_each(|m| *m = false);
    }

    pub fn todo(&mut self) {
        self.marcados.iter_mut().for_each(|m| *m = true);
    }

    fn rango(&mut self, a: usize, b: usize) {
        let (a, b) = (a.min(b), a.max(b).min(self.marcados.len().saturating_sub(1)));
        for m in self.marcados.iter_mut().take(b + 1).skip(a) {
            *m = true;
        }
    }

    /// Un clic en `i`: solo, con Ctrl (cambia ese y nada más) o con Mayúsculas (del ancla a él).
    pub fn clic(&mut self, i: usize, ctrl: bool, mayus: bool) {
        if i >= self.marcados.len() {
            return;
        }
        if mayus {
            let a = self.ancla.unwrap_or(i);
            if !ctrl {
                self.nada();
            }
            self.rango(a, i);
            self.ancla = Some(a);
        } else if ctrl {
            self.marcados[i] = !self.marcados[i];
            self.ancla = Some(i);
        } else {
            self.nada();
            self.marcados[i] = true;
            self.ancla = Some(i);
        }
        self.cursor = Some(i);
    }

    /// El rectángulo: lo que estaba (con Ctrl) más lo que toca ahora.
    pub fn rectangulo(&mut self, antes: &[bool], dentro: &[usize], ctrl: bool) {
        for (k, m) in self.marcados.iter_mut().enumerate() {
            *m = ctrl && antes.get(k).copied().unwrap_or(false);
        }
        for &i in dentro {
            if let Some(m) = self.marcados.get_mut(i) {
                *m = if ctrl { !antes.get(i).copied().unwrap_or(false) } else { true };
            }
        }
        if let Some(&i) = dentro.last() {
            self.cursor = Some(i);
            self.ancla = dentro.first().copied();
        }
    }

    /// El cursor pasa a `i` con una tecla: solo él elegido; con Mayúsculas, del ancla a él; con Ctrl, solo se mueve.
    pub fn mover(&mut self, i: usize, ctrl: bool, mayus: bool) {
        if i >= self.marcados.len() {
            return;
        }
        if mayus {
            let a = self.ancla.or(self.cursor).unwrap_or(i);
            self.nada();
            self.rango(a, i);
            self.ancla = Some(a);
        } else if !ctrl {
            self.nada();
            self.marcados[i] = true;
            self.ancla = Some(i);
        }
        self.cursor = Some(i);
    }
}

/// Las teclas que mueven el cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paso {
    Izquierda,
    Derecha,
    Arriba,
    Abajo,
    Inicio,
    Fin,
    PaginaArriba,
    PaginaAbajo,
}

/// A dónde va el cursor desde `actual` (o desde ninguno) con `n` entradas en `cols` columnas y `filas` filas a la vista.
pub fn mover_cursor(actual: Option<usize>, p: Paso, n: usize, cols: usize, filas: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let cols = cols.max(1);
    let Some(a) = actual else {
        return Some(if matches!(p, Paso::Fin) { n - 1 } else { 0 });
    };
    let a = a.min(n - 1);
    Some(match p {
        Paso::Izquierda => a.saturating_sub(1),
        Paso::Derecha => (a + 1).min(n - 1),
        Paso::Arriba => if a >= cols { a - cols } else { a },
        Paso::Abajo => if a + cols < n { a + cols } else if a / cols < (n - 1) / cols { n - 1 } else { a },
        Paso::Inicio => 0,
        Paso::Fin => n - 1,
        Paso::PaginaArriba => a.saturating_sub(cols * filas.max(1)),
        Paso::PaginaAbajo => (a + cols * filas.max(1)).min(n - 1),
    })
}

/// La búsqueda al escribir con el foco en la carpeta (sin la barra): la primera que empieza así, desde el cursor.
pub fn buscar_al_escribir(nombres: &[&str], busca: &str, desde: usize) -> Option<usize> {
    let b = plano(busca);
    if b.is_empty() || nombres.is_empty() {
        return None;
    }
    let n = nombres.len();
    (0..n).map(|k| (desde + k) % n).find(|&i| plano(nombres[i]).starts_with(&b))
}

// ---------------------------------------------------------------------------------------------------------------
// Lugares y caminos.

/// Dónde está un cajón: por qué raíz y por qué nombres (todos válidos).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lugar {
    pub raiz: u32,
    pub nombres: Vec<String>,
}

impl Lugar {
    pub fn raiz(raiz: u32) -> Self {
        Lugar { raiz, nombres: Vec::new() }
    }

    pub fn hijo(&self, n: &str) -> Self {
        let mut nombres = self.nombres.clone();
        nombres.push(String::from(n));
        Lugar { raiz: self.raiz, nombres }
    }

    pub fn padre(&self) -> Option<Self> {
        let mut l = self.clone();
        l.nombres.pop()?;
        Some(l)
    }

    /// Si `self` es `otro` o está dentro de él (por la misma raíz).
    pub fn dentro_de(&self, otro: &Lugar) -> bool {
        self.raiz == otro.raiz && self.nombres.len() >= otro.nombres.len() && self.nombres[..otro.nombres.len()] == otro.nombres[..]
    }

    /// Como texto, con el nombre de la raíz: "Home/Privado".
    pub fn texto(&self, raiz: &str) -> String {
        let mut t = String::from(raiz);
        for n in &self.nombres {
            t.push('/');
            t.push_str(n);
        }
        t
    }

    pub fn valido(&self) -> bool {
        self.nombres.len() <= MAX_HONDO && self.nombres.iter().all(|n| nombre_valido(n))
    }
}

/// Lo más hondo que se baja (al abrir, copiar o buscar): más, y se para.
pub const MAX_HONDO: usize = 32;

/// Por qué no vale un camino escrito: (en castellano, para la consola; en el idioma del usuario, para la ventana).
pub type Porque = (&'static str, &'static str);

/// El camino que se escribe en la barra (Ctrl+L): "Home/Privado", "Home:Privado/x", "Sistema" o, sin raíz, relativo
/// a `actual` ("Privado/x"). Cada parte tiene que ser un nombre válido: sin "..", sin ".", sin control. Las raíces
/// son `(nombre, número)` y no distinguen mayúsculas. Una barra al final no cuenta. El porqué de un error va dos veces:
/// en castellano (para la consola) y en el idioma del usuario (para la ventana).
pub fn interpretar_camino(texto: &str, raices: &[(&str, u32)], actual: &Lugar) -> Result<Lugar, Porque> {
    let t = texto.trim();
    if t.is_empty() {
        return Err(("Escribe un camino", crate::tr(108, "Escribe un camino")));
    }
    if t.chars().any(char::is_control) {
        return Err(("El camino tiene letras que no valen", crate::tr(109, "El camino tiene letras que no valen")));
    }
    // "Raíz:resto" o "Raíz/resto" o "Raíz".
    let (primera, resto) = match t.split_once(':') {
        Some((r, resto)) => (Some(r), resto),
        None => match t.split_once('/') {
            Some((r, resto)) => (Some(r), resto),
            None => (Some(t), ""),
        },
    };
    let raiz = primera.and_then(|r| raices.iter().find(|(n, _)| plano(n) == plano(r.trim())).map(|x| x.1));
    let (mut lugar, partes) = match raiz {
        Some(r) => (Lugar::raiz(r), resto),
        None if t.contains(':') => return Err(("No hay ningún lugar con ese nombre", crate::tr(110, "No hay ningún lugar con ese nombre"))),
        None => (actual.clone(), t),
    };
    for (k, p) in partes.split('/').enumerate() {
        let ultima = k == partes.split('/').count() - 1;
        if p.is_empty() && (ultima || k == 0 && partes.is_empty()) {
            continue;
        }
        if !nombre_valido(p) {
            return Err(if p == ".." {
                ("No se puede subir con \"..\": usa Atrás o Arriba", crate::tr(111, "No se puede subir con \"..\": usa Atrás o Arriba"))
            } else {
                ("Una parte del camino no vale", crate::tr(112, "Una parte del camino no vale"))
            });
        }
        lugar.nombres.push(String::from(p));
        if lugar.nombres.len() > MAX_HONDO {
            return Err(("El camino es demasiado largo", crate::tr(113, "El camino es demasiado largo")));
        }
    }
    Ok(lugar)
}

// ---------------------------------------------------------------------------------------------------------------
// Nombres nuevos.

/// Separa "nota.txt" en ("nota", ".txt"); una carpeta o un nombre sin extensión, entero.
pub fn base_y_extension(n: &str, cajon: bool) -> (&str, &str) {
    if cajon {
        return (n, "");
    }
    match n.rfind('.') {
        Some(i) if i > 0 && n.len() - i <= 9 => (&n[..i], &n[i..]),
        _ => (n, ""),
    }
}

/// Un nombre que no está en el cajón para la copia de `n`: "nota (copia).txt", "nota (copia 2).txt"...; si `n` no
/// está, el mismo. Nunca pasa de 200 bytes (se corta la base).
pub fn nombre_libre(n: &str, cajon: bool, existe: impl Fn(&str) -> bool, sufijo: &str) -> String {
    if !existe(n) {
        return String::from(n);
    }
    let (base, ext) = base_y_extension(n, cajon);
    for k in 1..10_000 {
        let s = match (sufijo.is_empty(), k) {
            (true, _) => alloc::format!(" ({})", k + 1),
            (false, 1) => alloc::format!(" ({})", sufijo),
            (false, _) => alloc::format!(" ({} {})", sufijo, k),
        };
        let mut b = String::from(base);
        while b.len() + s.len() + ext.len() > 200 {
            b.pop();
        }
        let nuevo = alloc::format!("{}{}{}", b, s, ext);
        if !existe(&nuevo) {
            return nuevo;
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------------------------------------------
// La papelera: un cajón ".Papelera" en la raíz de Home, con "archivos" (lo tirado, con un nombre que no se repite) y
// "info" (una ficha por cada uno, con el mismo nombre: de dónde vino).

pub const PAPELERA: &str = ".Papelera";
pub const PAPELERA_ARCHIVOS: &str = "archivos";
pub const PAPELERA_INFO: &str = "info";

/// La ficha de algo tirado: de qué raíz y cajón vino, con qué nombre y cuándo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ficha {
    pub origen: Lugar,
    pub nombre: String,
    pub fecha: u64,
}

impl Ficha {
    pub fn texto(&self) -> String {
        let mut t = alloc::format!("dashos-papelera 1\nraiz {}\nfecha {}\nnombre {}\n", self.origen.raiz, self.fecha, self.nombre);
        for n in &self.origen.nombres {
            t.push_str("cajon ");
            t.push_str(n);
            t.push('\n');
        }
        t
    }

    /// Lee una ficha; si algo no vale (un nombre raro, una raíz que no es de `raices`, de más), nada.
    pub fn leer(datos: &[u8], raices: &[u32]) -> Option<Ficha> {
        let t = core::str::from_utf8(datos).ok()?;
        let mut lineas = t.lines();
        if lineas.next()? != "dashos-papelera 1" {
            return None;
        }
        let (mut raiz, mut fecha, mut nombre, mut nombres) = (None, 0, None, Vec::new());
        for l in lineas {
            let (k, v) = l.split_once(' ')?;
            match k {
                "raiz" => raiz = Some(v.parse::<u32>().ok().filter(|r| raices.contains(r))?),
                "fecha" => fecha = v.parse().ok()?,
                "nombre" if nombre_valido(v) => nombre = Some(String::from(v)),
                "cajon" if nombre_valido(v) && nombres.len() < MAX_HONDO => nombres.push(String::from(v)),
                _ => return None,
            }
        }
        Some(Ficha { origen: Lugar { raiz: raiz?, nombres }, nombre: nombre?, fecha })
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Textos.

/// Un tamaño, para la ventana (en el idioma del usuario).
pub fn tamano(b: u64) -> String {
    match b {
        1 => String::from(crate::tr(114, "1 byte")),
        0..1000 => crate::trf(115, "{} bytes", &[&b]),
        1000..1_000_000 => crate::trf(116, "{},{} kB", &[&(b / 1000), &(b % 1000 / 100)]),
        1_000_000..1_000_000_000 => crate::trf(117, "{},{} MB", &[&(b / 1_000_000), &(b % 1_000_000 / 100_000)]),
        _ => crate::trf(118, "{},{} GB", &[&(b / 1_000_000_000), &(b % 1_000_000_000 / 100_000_000)]),
    }
}

/// Un tamaño, para la consola: siempre en castellano.
pub fn tamano_fijo(b: u64) -> String {
    match b {
        1 => String::from("1 byte"),
        0..1000 => alloc::format!("{} bytes", b),
        1000..1_000_000 => alloc::format!("{},{} kB", b / 1000, b % 1000 / 100),
        1_000_000..1_000_000_000 => alloc::format!("{},{} MB", b / 1_000_000, b % 1_000_000 / 100_000),
        _ => alloc::format!("{},{} GB", b / 1_000_000_000, b % 1_000_000_000 / 100_000_000),
    }
}

/// Año, mes y día de un número de días desde el 1 de enero de 1970 (calendario gregoriano, por eras de 400 años).
pub fn dia_civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// El nombre corto de un mes (de 1 a 12).
fn mes(m: i64) -> &'static str {
    match m {
        1 => crate::tr(119, "ene"),
        2 => crate::tr(120, "feb"),
        3 => crate::tr(121, "mar"),
        4 => crate::tr(122, "abr"),
        5 => crate::tr(123, "may"),
        6 => crate::tr(124, "jun"),
        7 => crate::tr(125, "jul"),
        8 => crate::tr(126, "ago"),
        9 => crate::tr(127, "sep"),
        10 => crate::tr(128, "oct"),
        11 => crate::tr(129, "nov"),
        _ => crate::tr(130, "dic"),
    }
}

/// La fecha corta, como Nautilus: "10:04" si es de hoy, "12 nov" si es de este año, "12 nov 2025" si no (UTC: aún
/// no hay zona horaria). Sin fecha, una raya.
pub fn fecha(seg: u64, ahora: u64) -> String {
    if seg == 0 {
        return String::from("—");
    }
    let (y, m, d) = dia_civil((seg / 86_400) as i64);
    if ahora != 0 && seg / 86_400 == ahora / 86_400 {
        let min = seg % 86_400 / 60;
        return alloc::format!("{:02}:{:02}", min / 60, min % 60);
    }
    let (ya, _, _) = dia_civil((ahora / 86_400) as i64);
    if ahora != 0 && y == ya {
        alloc::format!("{} {}", d, mes(m))
    } else {
        alloc::format!("{} {} {}", d, mes(m), y)
    }
}

/// Lo que dice la barra de abajo de lo elegido: "«nota.txt» elegido (18 bytes)", "3 elementos elegidos (1,2 MB)"...
pub fn texto_seleccion(nombres: &[&str], bytes: u64, carpetas: usize) -> String {
    match nombres.len() {
        0 => String::new(),
        1 if carpetas == 1 => crate::trf(131, "\"{}\" elegida", &[&nombres[0]]),
        1 => crate::trf(132, "\"{}\" elegido ({})", &[&nombres[0], &tamano(bytes)]),
        n if carpetas == n => crate::trf(133, "{} carpetas elegidas", &[&n]),
        n if carpetas == 0 => crate::trf(134, "{} elementos elegidos ({})", &[&n, &tamano(bytes)]),
        n => crate::trf(135, "{} elementos elegidos ({} sin contar las carpetas)", &[&n, &tamano(bytes)]),
    }
}
