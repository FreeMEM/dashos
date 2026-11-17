//! S32: el plan de instalación. Se calcula entero antes de escribir un byte, se enseña con un dibujo del disco antes y
//! después, y es un fichero de texto: se puede guardar, revisar y pasar al instalador para instalar sin manos (la
//! ventana y la orden del Shell hacen lo mismo). Lleva la huella de la tabla que se leyó: aplicado a un disco que
//! cambió desde entonces, no se hace.
//!
//! Formato (una cosa por línea, `clave valor`; lo que empieza por `#` es comentario y el dibujo va así):
//!
//! ```text
//! dashos-plan 1
//! disco sata0
//! sectores 104857600
//! sector 512
//! huella 1a2b3c4d
//! modo hueco | disco-entero | reinstalar | encoger
//! encoger 3 61865983            (solo en modo encoger: la partición y su nuevo final, exclusivo)
//! esp existente 1               (o: esp nueva <desde> <hasta> <guid>)
//! dashos <desde> <hasta> <guid>
//! disco-guid <guid>             (solo en disco entero)
//! nvram si
//! ```

use super::disco::{guid_de_texto, guid_texto};
use super::gpt::{Clase, Particion, Tabla, tipos};
use super::reconocer::Contenido;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

/// Lo mínimo para DashOS (pendiente del usuario: el tamaño recomendado).
pub const MINIMO_DASHOS: u64 = 4 << 30;
/// La ESP nueva, cuando hay que hacerla (disco entero, o un disco sin ESP).
pub const TAMANO_ESP: u64 = 260 << 20;
/// Lo que se deja libre en un NTFS al encogerlo, además de lo usado (el margen para Windows; pendiente del usuario).
pub const MARGEN_WINDOWS: u64 = 10 << 30;

/// Un disco tal como lo ve el instalador.
#[derive(Clone, Debug)]
pub struct DiscoVisto {
    /// Su nombre corto (`nvme0`, `sata1`).
    pub nombre: String,
    /// El modelo, en llano.
    pub modelo: String,
    pub sectores: u64,
    pub sector: usize,
    pub tabla: Tabla,
    /// Lo que hay en cada partición de `tabla.particiones`, en el mismo orden.
    pub contenidos: Vec<Contenido>,
    /// Si es el disco del que arrancó DashOS (el pendrive): no se ofrece.
    pub de_arranque: bool,
}

/// Lo que se pide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pedido {
    /// El hueco libre más grande (o el que empieza en ese sector).
    Hueco(Option<u64>),
    /// Borrar el disco entero.
    DiscoEntero,
    /// Encoger esa partición (NTFS) para hacer sitio, dejando a DashOS esos bytes (0: lo que se pueda).
    Encoger(u32, u64),
    /// Sobre la partición de DashOS que ya hay.
    Reinstalar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modo {
    Hueco,
    DiscoEntero,
    Reinstalar,
    Encoger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Esp {
    /// La que hay, por su número en la tabla.
    Existente(u32),
    /// Una nueva: [desde, hasta) y su GUID.
    Nueva(u64, u64, [u8; 16]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub disco: String,
    pub sectores: u64,
    pub sector: usize,
    pub huella: u32,
    pub modo: Modo,
    /// (número de partición, nuevo final exclusivo en sectores).
    pub encoger: Option<(u32, u64)>,
    pub esp: Esp,
    /// [desde, hasta) y el GUID de la partición de DashOS.
    pub dashos: (u64, u64, [u8; 16]),
    pub disco_guid: Option<[u8; 16]>,
    pub nvram: bool,
}

/// Por qué no hay plan, en llano.
pub type Motivo = String;

fn mib(s: u64, ts: usize) -> u64 {
    s * ts as u64 >> 20
}

/// Un tamaño en llano (GB con un decimal, o MB).
pub fn tamano(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        let d = bytes * 10 / (1 << 30);
        format!("{},{} GB", d / 10, d % 10)
    } else {
        format!("{} MB", bytes >> 20)
    }
}

/// Lo que impide tocar algo de este disco, aunque sea solo para añadir: se mira antes que nada.
fn impedimentos(d: &DiscoVisto) -> Option<Motivo> {
    if d.de_arranque {
        return Some(String::from("es el disco desde el que ha arrancado DashOS (el pendrive)"));
    }
    match d.tabla.clase {
        Clase::GptHibrida => return Some(String::from("tiene un MBR híbrido: no se toca")),
        Clase::Mbr => return Some(String::from("tiene una tabla MBR (antigua): solo se puede instalar borrándolo entero")),
        _ => {}
    }
    None
}

/// Calcula el plan. `guid()` da GUID nuevos al azar (de la versión 4).
pub fn calcular(d: &DiscoVisto, pedido: Pedido, guid: &mut dyn FnMut() -> [u8; 16]) -> Result<Plan, Motivo> {
    let ts = d.sector;
    let alin = ((1u64 << 20) / ts as u64).max(1);
    let minimo = MINIMO_DASHOS / ts as u64;
    if d.de_arranque {
        return Err(String::from("es el disco desde el que ha arrancado DashOS (el pendrive): no se instala en él"));
    }
    let base = Plan {
        disco: d.nombre.clone(),
        sectores: d.sectores,
        sector: ts,
        huella: d.tabla.huella,
        modo: Modo::Hueco,
        encoger: None,
        esp: Esp::Existente(0),
        dashos: (0, 0, [0; 16]),
        disco_guid: None,
        nvram: true,
    };
    if pedido == Pedido::DiscoEntero {
        let t = super::gpt::tabla_vacia(d.sectores, ts, [0; 16]);
        let c = t.cabecera.unwrap();
        let esp = (c.primer_util, c.primer_util + TAMANO_ESP / ts as u64);
        let fin = (c.ultimo_util + 1) / alin * alin;
        if fin < esp.1 + minimo {
            return Err(format!("el disco es demasiado pequeño: hacen falta {} y tiene {}", tamano(MINIMO_DASHOS + TAMANO_ESP), tamano(d.sectores * ts as u64)));
        }
        return Ok(Plan { modo: Modo::DiscoEntero, esp: Esp::Nueva(esp.0, esp.1, guid()), dashos: (esp.1, fin, guid()), disco_guid: Some(guid()), ..base });
    }
    if let Some(m) = impedimentos(d) {
        return Err(m);
    }
    if d.tabla.clase == Clase::Ninguna {
        return Err(String::from("el disco no tiene tabla de particiones: solo se puede instalar borrándolo entero"));
    }
    if !d.tabla.escribible() {
        let mut m = String::from("no se escribe en este disco");
        if let Some(a) = d.tabla.avisos.first() {
            m = format!("{}: {}", m, a.trim_end_matches('.'));
        }
        return Err(m);
    }
    // La ESP de este disco, si tiene (una sola, FAT de verdad).
    let esps: Vec<&Particion> = d.tabla.particiones.iter().filter(|p| p.tipo == tipos::ESP).collect();
    let esp_existente = match esps.as_slice() {
        [] => None,
        [p] => {
            let k = d.tabla.particiones.iter().position(|q| q.numero == p.numero).unwrap();
            if !matches!(d.contenidos.get(k), Some(Contenido::Fat { bits: 16 | 32, .. })) {
                return Err(String::from("la partición de arranque EFI no es FAT16 ni FAT32: no se toca"));
            }
            Some(p.numero)
        }
        _ => return Err(String::from("tiene más de una partición de arranque EFI: no se elige cuál")),
    };
    let dashos: Vec<&Particion> = d.tabla.particiones.iter().filter(|p| p.tipo == tipos::DASHOS).collect();
    match pedido {
        Pedido::Reinstalar => {
            let [p] = dashos.as_slice() else { return Err(String::from("no hay una (y solo una) partición de DashOS")) };
            let Some(e) = esp_existente else { return Err(String::from("el disco no tiene partición de arranque EFI")) };
            Ok(Plan { modo: Modo::Reinstalar, esp: Esp::Existente(e), dashos: (p.desde, p.hasta, p.guid), ..base })
        }
        Pedido::Hueco(desde) => {
            if !dashos.is_empty() && d.tabla.copias != super::gpt::Copias::SoloDashOS {
                return Err(String::from("ya hay una partición de DashOS en este disco: elige reinstalar"));
            }
            // Con una instalación cortada (SoloDashOS), la entrada a medias se quita: el hueco se mira sin ella.
            let mut t = d.tabla.clone();
            t.particiones.retain(|p| p.tipo != tipos::DASHOS);
            let huecos = t.huecos(alin);
            let hueco = match desde {
                Some(s) => huecos.iter().copied().find(|h| h.0 == s),
                None => huecos.iter().copied().max_by_key(|h| h.1 - h.0),
            };
            let Some((a, b)) = hueco else { return Err(String::from("no hay hueco libre en el disco")) };
            let nueva_esp = if esp_existente.is_none() { TAMANO_ESP / ts as u64 } else { 0 };
            if b - a < minimo + nueva_esp {
                return Err(format!(
                    "no hay sitio: el hueco libre más grande es de {} y DashOS necesita {}",
                    tamano((b - a) * ts as u64),
                    tamano(MINIMO_DASHOS + nueva_esp * ts as u64)
                ));
            }
            let esp = match esp_existente {
                Some(n) => Esp::Existente(n),
                None => Esp::Nueva(a, a + nueva_esp, guid()),
            };
            Ok(Plan { modo: Modo::Hueco, esp, dashos: (a + nueva_esp, b, guid()), ..base })
        }
        Pedido::Encoger(numero, quiere) => {
            if !dashos.is_empty() {
                return Err(String::from("ya hay una partición de DashOS en este disco: elige reinstalar"));
            }
            let Some(k) = d.tabla.particiones.iter().position(|p| p.numero == numero) else { return Err(String::from("esa partición no existe")) };
            let p = &d.tabla.particiones[k];
            let ntfs = match d.contenidos.get(k) {
                Some(Contenido::Ntfs(n)) => n,
                Some(Contenido::BitLocker) => return Err(String::from("la partición está cifrada con BitLocker: no se puede encoger desde aquí. Desactiva BitLocker en Windows, o encógela desde Windows (Administración de discos)")),
                _ => return Err(String::from("solo se encogen particiones de Windows (NTFS)")),
            };
            if ntfs.hibernado != Some(false) {
                return Err(String::from(
                    "Windows no se apagó del todo (está hibernado o con el inicio rápido): arranca Windows, apaga el inicio rápido y apágalo del todo, y vuelve",
                ));
            }
            if ntfs.sucio != Some(false) {
                return Err(String::from("Windows tiene una comprobación de disco pendiente: arranca Windows, deja que la haga y vuelve"));
            }
            let Some(usados) = ntfs.usados else { return Err(String::from("no se pudo saber cuánto ocupa Windows")) };
            let ocupado = usados * ntfs.cluster as u64;
            // Lo que se queda Windows: lo que usa más su margen, redondeado al MiB.
            let queda = (ocupado + MARGEN_WINDOWS).div_ceil(1 << 20) << 20;
            let tam = (p.hasta - p.desde) * ts as u64;
            if queda >= tam {
                return Err(format!("Windows está casi lleno: usa {} de {} y se le deja un margen de {}", tamano(ocupado), tamano(tam), tamano(MARGEN_WINDOWS)));
            }
            let nuevo_fin = (p.desde + queda / ts as u64).div_ceil(alin) * alin;
            // El hueco que queda tras el NTFS encogido, hasta la siguiente partición.
            let siguiente = d.tabla.particiones.iter().filter(|q| q.desde >= p.hasta).map(|q| q.desde).min().unwrap_or(d.tabla.cabecera.unwrap().ultimo_util + 1);
            let fin_hueco = siguiente / alin * alin;
            let nueva_esp = if esp_existente.is_none() { TAMANO_ESP / ts as u64 } else { 0 };
            let mut desde_dashos = nuevo_fin + nueva_esp;
            if quiere > 0 {
                let q = quiere.div_ceil(ts as u64).div_ceil(alin) * alin;
                if fin_hueco < q + nueva_esp {
                    return Err(String::from("no cabe lo pedido"));
                }
                desde_dashos = desde_dashos.max(fin_hueco - q);
            }
            if fin_hueco <= desde_dashos || fin_hueco - desde_dashos < minimo {
                return Err(format!("no hay sitio: encogiendo Windows quedarían {}, y DashOS necesita {}", tamano(fin_hueco.saturating_sub(nuevo_fin + nueva_esp) * ts as u64), tamano(MINIMO_DASHOS)));
            }
            let fin_ntfs = desde_dashos - nueva_esp;
            let esp = match esp_existente {
                Some(n) => Esp::Existente(n),
                None => Esp::Nueva(fin_ntfs, fin_ntfs + nueva_esp, guid()),
            };
            Ok(Plan { modo: Modo::Encoger, encoger: Some((numero, fin_ntfs)), esp, dashos: (desde_dashos, fin_hueco, guid()), ..base })
        }
        Pedido::DiscoEntero => unreachable!(),
    }
}

/// Un tramo del dibujo: [desde, hasta), su nombre y si es de DashOS.
struct Tramo {
    desde: u64,
    hasta: u64,
    nombre: String,
}

/// El dibujo de una fila (64 columnas) y su leyenda.
fn dibujar(out: &mut String, titulo: &str, tramos: &[Tramo], sectores: u64, ts: usize) {
    const ANCHO: u64 = 64;
    let mut fila = [b'.'; ANCHO as usize];
    let letras = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    // Cada tramo, al menos una columna, y sin tapar al de antes (los pequeños, como la ESP, se ven).
    let mut ocupada = 0usize;
    for (i, t) in tramos.iter().enumerate() {
        let a = ((t.desde as u128 * ANCHO as u128 / sectores as u128) as usize).max(ocupada).min(ANCHO as usize - 1);
        let b = (((t.hasta as u128 * ANCHO as u128).div_ceil(sectores as u128)) as usize).max(a + 1).min(ANCHO as usize);
        ocupada = b;
        for c in &mut fila[a..b] {
            *c = letras[i % 26];
        }
    }
    let _ = writeln!(out, "# {}:", titulo);
    let _ = writeln!(out, "#   [{}]", core::str::from_utf8(&fila).unwrap_or(""));
    for (i, t) in tramos.iter().enumerate() {
        let _ = writeln!(out, "#   {} {} ({})", letras[i % 26] as char, t.nombre, tamano((t.hasta - t.desde) * ts as u64));
    }
    let _ = writeln!(out, "#   . libre");
}

/// El nombre en llano de una partición con lo que tiene dentro.
pub fn nombre_particion(p: &Particion, c: Option<&Contenido>) -> String {
    let tipo = if p.mbr { "MBR" } else { tipos::nombre(&p.tipo) };
    let mut s = format!("{}: {}", p.numero, tipo);
    if let Some(c) = c {
        let _ = write!(s, ", {}", c.nombre());
        match c {
            Contenido::Ntfs(n) if !n.etiqueta.is_empty() => {
                let _ = write!(s, " \"{}\"", n.etiqueta);
            }
            Contenido::Fat { etiqueta, .. } | Contenido::Ext { etiqueta, .. } if !etiqueta.is_empty() && etiqueta != "NO NAME" => {
                let _ = write!(s, " \"{}\"", etiqueta);
            }
            _ => {}
        }
    }
    if !p.nombre.is_empty() {
        let _ = write!(s, " ({})", p.nombre);
    }
    s
}

/// El plan en texto, con su dibujo antes y después y lo que se pierde.
pub fn texto(p: &Plan, d: &DiscoVisto) -> String {
    let ts = p.sector;
    let mut s = String::new();
    let _ = writeln!(s, "# Plan de instalación de DashOS (S32). Revísalo: hasta que lo confirmes no se escribe nada.");
    let _ = writeln!(s, "# Disco {} ({}), {}.", d.nombre, d.modelo, tamano(d.sectores * ts as u64));
    let mut antes: Vec<Tramo> = Vec::new();
    for (k, q) in d.tabla.particiones.iter().enumerate() {
        antes.push(Tramo { desde: q.desde, hasta: q.hasta, nombre: nombre_particion(q, d.contenidos.get(k)) });
    }
    antes.sort_by_key(|t| t.desde);
    let mut despues: Vec<Tramo> = Vec::new();
    match p.modo {
        Modo::DiscoEntero => {}
        _ => {
            for (k, q) in d.tabla.particiones.iter().enumerate() {
                if q.tipo == tipos::DASHOS && p.modo != Modo::Reinstalar {
                    continue;
                }
                if q.tipo == tipos::DASHOS {
                    continue;
                }
                let mut hasta = q.hasta;
                let mut nombre = nombre_particion(q, d.contenidos.get(k));
                if let Some((n, f)) = p.encoger {
                    if n == q.numero {
                        hasta = f;
                        nombre = format!("{}, encogida", nombre);
                    }
                }
                despues.push(Tramo { desde: q.desde, hasta, nombre });
            }
        }
    }
    if let Esp::Nueva(a, b, _) = p.esp {
        despues.push(Tramo { desde: a, hasta: b, nombre: String::from("nueva: partición de arranque EFI, FAT32") });
    }
    despues.push(Tramo { desde: p.dashos.0, hasta: p.dashos.1, nombre: String::from("nueva: DashOS, DashFS") });
    despues.sort_by_key(|t| t.desde);
    dibujar(&mut s, "Antes", &antes, d.sectores, ts);
    dibujar(&mut s, "Después", &despues, d.sectores, ts);
    let _ = writeln!(s, "# Lo que se hace:");
    match p.modo {
        Modo::DiscoEntero => {
            let _ = writeln!(s, "#   - SE BORRA EL DISCO ENTERO: todo lo que tiene se pierde y no se puede recuperar.");
        }
        Modo::Reinstalar => {
            let _ = writeln!(s, "#   - Se borra la partición de DashOS que hay y se instala de nuevo: lo que tenía se pierde.");
        }
        Modo::Encoger => {
            let (n, f) = p.encoger.unwrap();
            let q = d.tabla.particiones.iter().find(|q| q.numero == n).unwrap();
            let _ = writeln!(s, "#   - Se encoge la partición {} de Windows de {} a {} (sus ficheros no se tocan).", n, tamano((q.hasta - q.desde) * ts as u64), tamano((f - q.desde) * ts as u64));
        }
        Modo::Hueco => {
            let _ = writeln!(s, "#   - Se usa el espacio libre: ninguna partición de antes cambia.");
        }
    }
    match p.esp {
        Esp::Existente(n) => {
            let _ = writeln!(s, "#   - En la partición de arranque EFI ({}) se añade la carpeta EFI\\DashOS; lo demás no se toca.", n);
        }
        Esp::Nueva(..) => {
            let _ = writeln!(s, "#   - Se crea una partición de arranque EFI con el cargador de DashOS.");
        }
    }
    let _ = writeln!(s, "#   - Se crea la partición de DashOS de {} y se copia el sistema.", tamano((p.dashos.1 - p.dashos.0) * ts as u64));
    if p.nvram {
        let _ = writeln!(s, "#   - Se añade \"DashOS\" al menú de arranque del ordenador (al final: el que arranca ahora sigue igual), y el");
        let _ = writeln!(s, "#     próximo arranque será DashOS, una vez.");
    }
    let _ = writeln!(s, "#   - Antes de escribir se guarda una copia de las tablas de particiones.");
    for a in &d.tabla.avisos {
        let _ = writeln!(s, "# Aviso: {}", a);
    }
    for (k, c) in d.contenidos.iter().enumerate() {
        if let Contenido::Ntfs(n) = c {
            if n.hibernado == Some(true) {
                let _ = writeln!(s, "# Aviso: el Windows de la partición {} está hibernado (o con el inicio rápido): no se toca.", d.tabla.particiones[k].numero);
            }
        }
        if let Contenido::BitLocker = c {
            let _ = writeln!(s, "# Aviso: la partición {} está cifrada con BitLocker: no se toca.", d.tabla.particiones[k].numero);
        }
    }
    let _ = writeln!(s, "dashos-plan 1");
    let _ = writeln!(s, "disco {}", p.disco);
    let _ = writeln!(s, "sectores {}", p.sectores);
    let _ = writeln!(s, "sector {}", p.sector);
    let _ = writeln!(s, "huella {:08x}", p.huella);
    let modo = match p.modo {
        Modo::Hueco => "hueco",
        Modo::DiscoEntero => "disco-entero",
        Modo::Reinstalar => "reinstalar",
        Modo::Encoger => "encoger",
    };
    let _ = writeln!(s, "modo {}", modo);
    if let Some((n, f)) = p.encoger {
        let _ = writeln!(s, "encoger {} {}", n, f);
    }
    let mut g = String::new();
    match p.esp {
        Esp::Existente(n) => {
            let _ = writeln!(s, "esp existente {}", n);
        }
        Esp::Nueva(a, b, guid) => {
            guid_texto(&guid, &mut g);
            let _ = writeln!(s, "esp nueva {} {} {}", a, b, g);
        }
    }
    g.clear();
    guid_texto(&p.dashos.2, &mut g);
    let _ = writeln!(s, "dashos {} {} {}", p.dashos.0, p.dashos.1, g);
    if let Some(dg) = p.disco_guid {
        g.clear();
        guid_texto(&dg, &mut g);
        let _ = writeln!(s, "disco-guid {}", g);
    }
    let _ = writeln!(s, "nvram {}", if p.nvram { "si" } else { "no" });
    let _ = mib;
    s
}

/// Lee un plan en texto. Desconfía: cualquier línea que no se entienda, o que falte algo, y no hay plan.
pub fn leer(t: &str) -> Result<Plan, Motivo> {
    let mut p = Plan {
        disco: String::new(),
        sectores: 0,
        sector: 0,
        huella: 0,
        modo: Modo::Hueco,
        encoger: None,
        esp: Esp::Existente(0),
        dashos: (0, 0, [0; 16]),
        disco_guid: None,
        nvram: true,
    };
    let mut visto = 0u32;
    let num = |s: Option<&str>| s.and_then(|x| x.parse::<u64>().ok()).ok_or_else(|| String::from("número que no vale"));
    let guid = |s: Option<&str>| s.and_then(guid_de_texto).ok_or_else(|| String::from("GUID que no vale"));
    if t.len() > 64 * 1024 {
        return Err(String::from("el plan es demasiado largo"));
    }
    for linea in t.lines() {
        let l = linea.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let mut c = l.split_ascii_whitespace();
        let clave = c.next().unwrap();
        let bit = match clave {
            "dashos-plan" => {
                if c.next() != Some("1") {
                    return Err(String::from("versión de plan desconocida"));
                }
                1
            }
            "disco" => {
                let n = c.next().ok_or("falta el disco")?;
                if n.len() > 16 || !n.bytes().all(|b| b.is_ascii_alphanumeric()) {
                    return Err(String::from("nombre de disco que no vale"));
                }
                p.disco = String::from(n);
                2
            }
            "sectores" => {
                p.sectores = num(c.next())?;
                4
            }
            "sector" => {
                p.sector = num(c.next())? as usize;
                8
            }
            "huella" => {
                p.huella = c.next().and_then(|x| u32::from_str_radix(x, 16).ok()).ok_or("huella que no vale")?;
                16
            }
            "modo" => {
                p.modo = match c.next() {
                    Some("hueco") => Modo::Hueco,
                    Some("disco-entero") => Modo::DiscoEntero,
                    Some("reinstalar") => Modo::Reinstalar,
                    Some("encoger") => Modo::Encoger,
                    _ => return Err(String::from("modo desconocido")),
                };
                32
            }
            "encoger" => {
                p.encoger = Some((num(c.next())? as u32, num(c.next())?));
                64
            }
            "esp" => {
                p.esp = match c.next() {
                    Some("existente") => Esp::Existente(num(c.next())? as u32),
                    Some("nueva") => Esp::Nueva(num(c.next())?, num(c.next())?, guid(c.next())?),
                    _ => return Err(String::from("esp que no vale")),
                };
                128
            }
            "dashos" => {
                p.dashos = (num(c.next())?, num(c.next())?, guid(c.next())?);
                256
            }
            "disco-guid" => {
                p.disco_guid = Some(guid(c.next())?);
                512
            }
            "nvram" => {
                p.nvram = match c.next() {
                    Some("si") => true,
                    Some("no") => false,
                    _ => return Err(String::from("nvram: si o no")),
                };
                1024
            }
            _ => return Err(format!("línea que no se entiende: {}", clave)),
        };
        if c.next().is_some() {
            return Err(format!("sobra algo en la línea de {}", clave));
        }
        if visto & bit != 0 {
            return Err(format!("{} repetido", clave));
        }
        visto |= bit;
    }
    let obligatorio = 1 | 2 | 4 | 8 | 16 | 32 | 128 | 256 | 1024;
    if visto & obligatorio != obligatorio {
        return Err(String::from("al plan le falta algo"));
    }
    if (p.modo == Modo::Encoger) != p.encoger.is_some() || (p.modo == Modo::DiscoEntero) != p.disco_guid.is_some() {
        return Err(String::from("el plan no es coherente"));
    }
    if !(512..=4096).contains(&p.sector) || !p.sector.is_power_of_two() || p.dashos.0 >= p.dashos.1 || p.dashos.1 > p.sectores {
        return Err(String::from("el plan no es coherente"));
    }
    if let Esp::Nueva(a, b, _) = p.esp {
        if a >= b || b > p.sectores || (a < p.dashos.1 && p.dashos.0 < b) {
            return Err(String::from("el plan no es coherente"));
        }
    }
    Ok(p)
}

/// Comprueba que el plan casa con el disco tal como está ahora (que no cambió desde que se calculó) y que es lo que
/// saldría de calcularlo otra vez (no se fía de un plan escrito a mano que pise algo).
pub fn comprobar(p: &Plan, d: &DiscoVisto) -> Result<(), Motivo> {
    if p.disco != d.nombre || p.sectores != d.sectores || p.sector != d.sector {
        return Err(String::from("el plan es de otro disco"));
    }
    if p.huella != d.tabla.huella {
        return Err(String::from("el disco ha cambiado desde que se hizo el plan: hazlo de nuevo"));
    }
    if d.de_arranque {
        return Err(String::from("es el disco desde el que ha arrancado DashOS"));
    }
    // Se recalcula con lo mismo y se compara lo que importa (los GUID nuevos son los del plan).
    let pedido = match p.modo {
        Modo::Hueco => Pedido::Hueco(Some(match p.esp {
            Esp::Nueva(a, _, _) => a,
            Esp::Existente(_) => p.dashos.0,
        })),
        Modo::DiscoEntero => Pedido::DiscoEntero,
        Modo::Reinstalar => Pedido::Reinstalar,
        Modo::Encoger => Pedido::Encoger(p.encoger.unwrap().0, 0),
    };
    let mut guids = [p.dashos.2, p.dashos.2, p.dashos.2].into_iter();
    let otro = calcular(d, pedido, &mut || guids.next().unwrap_or([0; 16]))?;
    let ok = match p.modo {
        // En hueco y encoger el plan puede dejar a DashOS menos sitio que todo (empieza más tarde), nunca más.
        Modo::Hueco | Modo::Encoger => p.dashos.1 == otro.dashos.1 && p.dashos.0 >= otro.dashos.0 && p.dashos.1 - p.dashos.0 >= MINIMO_DASHOS / p.sector as u64,
        _ => p.dashos.0 == otro.dashos.0 && p.dashos.1 == otro.dashos.1,
    };
    let esp_ok = match (p.esp, otro.esp) {
        (Esp::Existente(a), Esp::Existente(b)) => a == b,
        (Esp::Nueva(a, b, _), Esp::Nueva(c, e, _)) => p.modo == Modo::Encoger || (a == c && b == e),
        _ => false,
    };
    let enc_ok = match (p.encoger, otro.encoger) {
        (None, None) => true,
        // Encoger: nunca por debajo de lo que dejaría el cálculo (lo usado más el margen).
        (Some((n, f)), Some((m, g))) => n == m && f >= g && f <= p.dashos.0,
        _ => false,
    };
    if !ok || !esp_ok || !enc_ok {
        return Err(String::from("el plan no casa con el disco: pisa algo o no deja sitio"));
    }
    Ok(())
}
