//! El motor de Paq: el almacén, las generaciones y las transacciones, sobre un `Disco`.
//!
//! Cada nivel (`Paquetes:`, lo de todos; `Home:Paquetes`, lo de cada usuario) tiene la misma forma:
//!
//! ```text
//! <raíz>/<Cajón>                          el cajón en uso de cada paquete: lo que ven la gente y los programas
//! <raíz>/Paq/Almacen/<nombre>/<hueco>.pkg   la ficha de cada versión guardada (no se cambia nunca)
//! <raíz>/Paq/Almacen/<nombre>/<hueco>.readme su manifiesto, tal cual
//! <raíz>/Paq/Almacen/<nombre>/<hueco>/      su cajón, mientras no está en uso
//! <raíz>/Paq/Generaciones/<n>               la tabla de cada generación (no se cambia nunca)
//! <raíz>/Paq/Asignaciones                   las asignaciones de la generación en uso, para `sesiones`
//! <raíz>/Paq/Series                         la última serie vista de cada índice (contra volver a uno viejo)
//! <raíz>/Paq/Cerrojo                        mientras trabaja un Paq
//! ```
//!
//! **La generación manda.** La de número más alto que cuadra con su suma es la que está en uso. Una transacción baja y
//! comprueba todo, lo guarda en el almacén (sin tocar nada de lo que se ve), escribe la generación siguiente de una vez
//! (eso es el cambio: atómico, lo da el sistema de archivos) y después pone a la vista lo que dice: cada cajón de una
//! versión está o en el almacén o a la vista, nunca en los dos, y se pasa de un sitio a otro cambiando de nombre (también
//! atómico). Así una versión cuyo cajón falta en el almacén es la que está a la vista. Si se corta a medias, al empezar
//! otra vez (el servicio, al arrancar) se termina de poner a la vista lo que dice la generación que manda: la de antes
//! entera, o la nueva entera; nunca media.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

use pkg2::formato::{Manifiesto, Opcion, Version};
use pkg2::plan::{self, Maquina, Paquete, Paso, Plan};
use pkg2::requisitos::Req;
use pkg2::resolutor::{self, Caso, Orden};

use crate::cambio::{Cambio, MAX_CAMBIOS};
use crate::config::{self, Clave, Repo};
use crate::disco::{self, Disco, NO_EXISTE, YA_EXISTE, borrar_todo, unir};
use crate::ficha::{Ficha, Fichero};
use crate::generacion::{Generacion, Instalado, Motivo};
use crate::indice::{self, Entrada, Indice};
use crate::texto::{self, bonita, hueco};
use crate::{ARQUITECTURA, Fallo, firma, gz, huella, tar};

/// El cajón de Paq en cada nivel: ningún paquete se puede llamar así.
pub const CAJON_PAQ: &str = "Paq";
/// Lo más que se lee de un índice (comprimido y sin comprimir), de una ficha o de un paquete descomprimido.
pub const MAX_INDICE_GZ: usize = 16 << 20;
pub const MAX_INDICE: usize = 64 << 20;
pub const MAX_TEXTO: usize = 256 * 1024;
pub const MAX_PAQUETE: usize = 64 << 20;
/// Lo que se guarda solo (S8): las tres últimas generaciones y las de los últimos 30 días.
pub const GUARDAR_ULTIMAS: usize = 3;
pub const GUARDAR_DIAS: u64 = 30;

/// Las asignaciones que un paquete no puede declarar: los volúmenes y las del sistema (S4b).
pub const ASIGNACIONES_DEL_SISTEMA: &[&str] = &[
    "SYS", "Home", "RAM", "Work", "PROGDIR", "Paquetes", "Programas", "Estado", "Usuarios", "C", "LIBS", "DEVS", "FONTS", "LOCALE", "S",
    "ENVARC", "CLASSES", "REXX", "L", "KEYMAPS", "PRINTERS", "HELP", "ENV", "T", "Datos",
];

/// Si `c` vale como cajón de un paquete: una sola parte, que no sea la de Paq ni un icono.
pub fn cajon_valido(c: &str) -> bool {
    tar::ruta_valida(c) && !c.contains('/') && c.len() <= 64 && !c.eq_ignore_ascii_case(CAJON_PAQ) && !c.to_ascii_lowercase().ends_with(".info")
}

/// Si `n` vale como nombre de una asignación de paquete: lo de `dashos::asignaciones::nombre_valido` sin las del sistema.
pub fn asignacion_valida(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 30
        && n.bytes().all(|c| c > b' ' && c != 0x7f && !b":/=,*@".contains(&c))
        && !ASIGNACIONES_DEL_SISTEMA.iter().any(|s| s.eq_ignore_ascii_case(n))
}

// ---------------------------------------------------------------------------------------------------------------
// El nivel y sus rutas.

#[derive(Clone, Debug)]
pub struct Nivel {
    /// `Paquetes:` o `Home:Paquetes`.
    pub raiz: String,
}

impl Nivel {
    pub fn de_todos() -> Nivel {
        Nivel { raiz: "Paquetes:".into() }
    }
    pub fn del_usuario() -> Nivel {
        Nivel { raiz: "Home:Paquetes".into() }
    }
    pub fn en(&self, rel: &str) -> String {
        unir(&self.raiz, rel)
    }
    pub fn paq(&self, rel: &str) -> String {
        self.en(&unir(CAJON_PAQ, rel))
    }
    fn almacen(&self, nombre: &str) -> String {
        self.paq(&unir("Almacen", nombre))
    }
    fn hueco(&self, nombre: &str, v: &Version) -> String {
        unir(&self.almacen(nombre), &hueco(v))
    }
    fn generacion(&self, n: u32) -> String {
        self.paq(&format!("Generaciones/{n}"))
    }
    /// Cómo se llama un cajón para las asignaciones (`Paquetes:Saludo`, `Home:Paquetes/Saludo`).
    pub fn a_la_vista(&self, cajon: &str) -> String {
        self.en(cajon)
    }
}

fn de_disco(e: i32, ruta: &str) -> Fallo {
    if e == disco::LLENO { Fallo::Lleno(format!("{ruta}: el disco está lleno")) } else { Fallo::Disco(e, ruta.to_string()) }
}

fn leer_texto(d: &mut dyn Disco, ruta: &str) -> Result<Option<String>, Fallo> {
    match d.leer(ruta, MAX_TEXTO) {
        Ok(b) => String::from_utf8(b).map(Some).map_err(|_| Fallo::Danado(format!("{ruta} no es texto"))),
        Err(NO_EXISTE) => Ok(None),
        Err(e) => Err(de_disco(e, ruta)),
    }
}

fn hacer(d: &mut dyn Disco, ruta: &str) -> Result<(), Fallo> {
    match d.hacer_cajon(ruta) {
        Ok(()) | Err(YA_EXISTE) => Ok(()),
        Err(e) => Err(de_disco(e, ruta)),
    }
}

fn listar_o_nada(d: &mut dyn Disco, ruta: &str) -> Result<Vec<(String, bool)>, Fallo> {
    match d.listar(ruta) {
        Ok(v) => Ok(v),
        Err(NO_EXISTE) => Ok(Vec::new()),
        Err(e) => Err(de_disco(e, ruta)),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Lo que hay: generaciones y almacén.

/// Una versión guardada en el almacén.
#[derive(Clone, Debug)]
pub struct Guardada {
    pub ficha: Ficha,
    pub manifiesto: Manifiesto,
    pub readme: String,
    /// Su cajón está a la vista (no en el almacén).
    pub en_uso: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Estado {
    /// La generación que manda (número 0 y vacía si aún no hay ninguna).
    pub generacion: Generacion,
    /// Todas las que valen, de la más vieja a la más nueva.
    pub generaciones: Vec<Generacion>,
    /// El número más alto que hay, valga o no: la siguiente lleva uno más.
    pub ultima: u32,
    /// Los números de las que no valen.
    pub danadas: Vec<u32>,
    pub almacen: Vec<Guardada>,
    /// Lo que conviene decir (generaciones dañadas, versiones en duda).
    pub avisos: Vec<String>,
}

impl Estado {
    pub fn guardada(&self, nombre: &str, v: &Version) -> Option<&Guardada> {
        self.almacen.iter().find(|g| g.ficha.nombre == nombre && g.ficha.version == *v)
    }
    pub fn instalado(&self, nombre: &str) -> Option<&Instalado> {
        self.generacion.buscar(nombre)
    }
}

/// Lee lo que hay en `nivel`, sin cambiar nada (lo puede hacer quien solo lee, como la orden con lo de todos).
pub fn cargar(d: &mut dyn Disco, nivel: &Nivel) -> Result<Estado, Fallo> {
    let mut e = Estado::default();
    let mut numeros: Vec<u32> = Vec::new();
    for (n, cajon) in listar_o_nada(d, &nivel.paq("Generaciones"))? {
        match pkg2::formato::numero(&n, u32::MAX as u64) {
            Some(x) if !cajon => numeros.push(x as u32),
            _ => {}
        }
    }
    numeros.sort_unstable();
    for n in numeros {
        e.ultima = e.ultima.max(n);
        let r = leer_texto(d, &nivel.generacion(n))?;
        match r.as_deref().map(Generacion::leer) {
            Some(Ok(g)) if g.numero == n => e.generaciones.push(g),
            _ => e.danadas.push(n),
        }
    }
    if let Some(g) = e.generaciones.last() {
        e.generacion = g.clone();
    }
    for &n in &e.danadas {
        if n > e.generacion.numero {
            e.avisos.push(format!("La generación {n} está dañada: se usa la {}", e.generacion.numero));
        }
    }
    for (paquete, cajon) in listar_o_nada(d, &nivel.paq("Almacen"))? {
        if !cajon {
            continue;
        }
        let dir = nivel.almacen(&paquete);
        let dentro = listar_o_nada(d, &dir)?;
        for (f, es_cajon) in &dentro {
            let Some(h) = f.strip_suffix(".pkg").filter(|_| !es_cajon) else { continue };
            let ficha = leer_texto(d, &unir(&dir, f))?.ok_or_else(|| Fallo::Disco(NO_EXISTE, f.clone()))?;
            let readme = leer_texto(d, &unir(&dir, &format!("{h}.readme")))?.unwrap_or_default();
            let ficha = match Ficha::leer(&ficha) {
                Ok(x) if x.es_su_manifiesto(&readme) && hueco(&x.version) == h && x.nombre == paquete => x,
                Ok(_) => {
                    e.avisos.push(format!("{paquete} {h} está en duda: su ficha no cuadra con su manifiesto"));
                    continue;
                }
                Err(por) => {
                    e.avisos.push(format!("{paquete} {h} está en duda: {por}"));
                    continue;
                }
            };
            let Ok(manifiesto) = pkg2::formato::leer(&readme, &format!("{paquete}.readme")) else {
                e.avisos.push(format!("{paquete} {h} está en duda: su manifiesto no se entiende"));
                continue;
            };
            let en_uso = !dentro.iter().any(|(x, c)| *c && x == h);
            e.almacen.push(Guardada { ficha, manifiesto, readme, en_uso });
        }
    }
    Ok(e)
}

/// Lo que queda a medias de una transacción cortada: cajones a medio desempaquetar (`.nuevo`), cajones sin su ficha
/// y manifiestos sueltos. Se borra.
fn barrer(d: &mut dyn Disco, nivel: &Nivel) -> Result<usize, Fallo> {
    let mut borrados = 0;
    for (paquete, cajon) in listar_o_nada(d, &nivel.paq("Almacen"))? {
        if !cajon {
            continue;
        }
        let dir = nivel.almacen(&paquete);
        let dentro = listar_o_nada(d, &dir)?;
        let tiene_ficha = |h: &str| dentro.iter().any(|(x, c)| !c && *x == format!("{h}.pkg"));
        for (f, es_cajon) in &dentro {
            let sobra = if *es_cajon {
                f.ends_with(".nuevo") || !tiene_ficha(f)
            } else {
                f.strip_suffix(".readme").is_some_and(|h| !tiene_ficha(h))
            };
            if sobra {
                let r = unir(&dir, f);
                borrar_todo(d, &r).map_err(|e| de_disco(e, &r))?;
                borrados += 1;
            }
        }
    }
    Ok(borrados)
}

/// Pone a la vista lo que dice la generación que manda, y escribe sus asignaciones. Devuelve cuántos cajones movió.
pub fn reconciliar(d: &mut dyn Disco, nivel: &Nivel, e: &mut Estado) -> Result<usize, Fallo> {
    let mut movidos = 0;
    let quiere: Vec<(String, Version)> = e.generacion.paquetes.iter().map(|p| (p.nombre.clone(), p.version.clone())).collect();
    for (n, v) in &quiere {
        if e.guardada(n, v).is_none() {
            return Err(Fallo::Danado(format!("falta {n} {} en el almacén; vuelve a instalarlo", bonita(v))));
        }
    }
    // Primero, lo que está a la vista y no toca, de vuelta al almacén.
    let mut perdidas = Vec::new();
    for i in 0..e.almacen.len() {
        let g = &e.almacen[i];
        if !g.en_uso || quiere.iter().any(|(n, v)| *n == g.ficha.nombre && *v == g.ficha.version) {
            continue;
        }
        let (de, a) = (nivel.en(&g.ficha.cajon), nivel.hueco(&g.ficha.nombre, &g.ficha.version));
        match d.renombrar(&de, &a) {
            Ok(()) => {}
            // Su cajón ya no está: esa versión se perdió. Como no se usa, se olvida.
            Err(NO_EXISTE) => {
                for ext in ["pkg", "readme"] {
                    let r = format!("{a}.{ext}");
                    let _ = d.borrar(&r);
                }
                perdidas.push(i);
            }
            Err(x) => return Err(de_disco(x, &de)),
        }
        e.almacen[i].en_uso = false;
        movidos += 1;
    }
    for i in perdidas.into_iter().rev() {
        let g = e.almacen.remove(i);
        e.avisos.push(format!("El cajón de {} {} se perdió y se olvida (no estaba en uso)", g.ficha.nombre, bonita(&g.ficha.version)));
    }
    // Después, lo que toca, a la vista.
    for (n, v) in &quiere {
        let i = e.almacen.iter().position(|g| g.ficha.nombre == *n && g.ficha.version == *v).unwrap();
        if e.almacen[i].en_uso {
            if d.mirar(&nivel.en(&e.almacen[i].ficha.cajon)).map_err(|x| de_disco(x, &nivel.raiz))?.is_none() {
                return Err(Fallo::Danado(format!("el cajón de {n} {} se perdió; vuelve a instalarlo", bonita(v))));
            }
            continue;
        }
        let g = &e.almacen[i];
        let a = nivel.en(&g.ficha.cajon);
        if d.mirar(&a).map_err(|x| de_disco(x, &a))?.is_some() {
            return Err(Fallo::Danado(format!("hay un cajón {a} que no es de Paq: apártalo y vuelve a probar")));
        }
        let de = nivel.hueco(n, v);
        d.renombrar(&de, &a).map_err(|x| de_disco(x, &de))?;
        e.almacen[i].en_uso = true;
        movidos += 1;
    }
    escribir_asignaciones(d, nivel, e)?;
    Ok(movidos)
}

/// El texto de las asignaciones de la generación en uso: una línea `NOMBRE=<raíz><cajón>[/camino]` por asignación,
/// como las de la tabla de nombres (`dashos::asignaciones`).
pub fn asignaciones(nivel: &Nivel, e: &Estado) -> String {
    let mut t = String::from("# Las asignaciones de los paquetes en uso. Lo escribe Paq; lo lee sesiones al montar los nombres.\n");
    for p in &e.generacion.paquetes {
        let Some(g) = e.guardada(&p.nombre, &p.version) else { continue };
        for (n, c) in &g.manifiesto.assigns {
            let n = n.trim_end_matches(':');
            // Relativo a donde se instaló (formato 2: como `Run`), así que empieza por su cajón.
            let destino = nivel.en(c);
            t += &format!("{n}={destino}\n");
        }
    }
    t
}

fn escribir_asignaciones(d: &mut dyn Disco, nivel: &Nivel, e: &Estado) -> Result<(), Fallo> {
    let ruta = nivel.paq("Asignaciones");
    let nuevo = asignaciones(nivel, e);
    if leer_texto(d, &ruta)?.as_deref() == Some(nuevo.as_str()) {
        return Ok(());
    }
    match d.borrar(&ruta) {
        Ok(()) | Err(NO_EXISTE) => {}
        Err(x) => return Err(de_disco(x, &ruta)),
    }
    d.guardar(&ruta, nuevo.as_bytes()).map_err(|x| de_disco(x, &ruta))
}

/// Lo que se hizo al poner en orden un nivel (al empezar una transacción, o el servicio al arrancar).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Arreglo {
    pub borrados: usize,
    pub movidos: usize,
}

/// Crea los cajones de Paq si faltan, borra lo que dejó a medias una transacción cortada y pone a la vista la
/// generación que manda. Hace falta poder escribir en el nivel.
pub fn poner_en_orden(d: &mut dyn Disco, nivel: &Nivel) -> Result<(Estado, Arreglo), Fallo> {
    for r in [nivel.raiz.clone(), nivel.paq(""), nivel.paq("Almacen"), nivel.paq("Generaciones")] {
        if !r.ends_with(':') {
            hacer(d, r.trim_end_matches('/'))?;
        }
    }
    let borrados = barrer(d, nivel)?;
    let mut e = cargar(d, nivel)?;
    let movidos = reconciliar(d, nivel, &mut e)?;
    Ok((e, Arreglo { borrados, movidos }))
}

// ---------------------------------------------------------------------------------------------------------------
// El cerrojo: un solo Paq a la vez en cada nivel.

/// Coge el cerrojo de `nivel` a nombre de `quien`. Si ya lo tiene otro, `Ocupado` con lo que dice el cerrojo.
pub fn cerrar(d: &mut dyn Disco, nivel: &Nivel, quien: &str) -> Result<(), Fallo> {
    for r in [nivel.raiz.clone(), nivel.paq("")] {
        if !r.ends_with(':') {
            hacer(d, r.trim_end_matches('/'))?;
        }
    }
    let ruta = nivel.paq("Cerrojo");
    match d.guardar(&ruta, format!("{quien}\n").as_bytes()) {
        Ok(()) => Ok(()),
        Err(YA_EXISTE) => {
            let quien = leer_texto(d, &ruta).ok().flatten().unwrap_or_default();
            Err(Fallo::Ocupado(format!("{} ({})", nivel.raiz, quien.trim())))
        }
        Err(x) => Err(de_disco(x, &ruta)),
    }
}

/// Quién tiene el cerrojo, si alguien.
pub fn quien_cierra(d: &mut dyn Disco, nivel: &Nivel) -> Option<String> {
    leer_texto(d, &nivel.paq("Cerrojo")).ok().flatten().map(|t| t.trim().to_string())
}

pub fn abrir(d: &mut dyn Disco, nivel: &Nivel) {
    let _ = d.borrar(&nivel.paq("Cerrojo"));
}

// ---------------------------------------------------------------------------------------------------------------
// Los repositorios y sus índices.

/// Un índice comprobado, con dónde está su repositorio.
#[derive(Clone, Debug)]
pub struct Fuente {
    pub repo: Repo,
    /// `<url>/aarch64-dashos`: de ahí cuelgan los paquetes.
    pub base: String,
    pub indice: Indice,
}

#[derive(Clone, Debug, Default)]
pub struct Repos {
    pub repos: Vec<Repo>,
    pub claves: Vec<Clave>,
    pub fuentes: Vec<Fuente>,
    /// Los repositorios cuyo índice no vale, y por qué (en llano).
    pub malos: Vec<(String, String)>,
    pub avisos: Vec<String>,
    /// La serie de cada índice que se usó (para apuntarla como la última vista).
    pub series: Vec<(String, u64)>,
}

impl Repos {
    pub fn entradas(&self) -> impl Iterator<Item = (&Fuente, &Entrada)> {
        self.fuentes.iter().flat_map(|f| f.indice.entradas.iter().map(move |e| (f, e)))
    }

    pub fn entrada(&self, nombre: &str, v: &Version, repo: &str) -> Option<(&Fuente, &Entrada)> {
        let mut es: Vec<_> = self.entradas().filter(|(f, e)| f.repo.nombre == repo && e.nombre == nombre && e.version == *v).collect();
        es.sort_by_key(|(_, e)| e.rank);
        es.into_iter().next()
    }
}

/// Las series vistas (`Paq/Series`: una línea `repositorio serie` por índice).
pub fn leer_series(d: &mut dyn Disco, nivel: &Nivel) -> Vec<(String, u64)> {
    let t = leer_texto(d, &nivel.paq("Series")).ok().flatten().unwrap_or_default();
    t.lines()
        .filter_map(|l| l.split_once(' '))
        .filter_map(|(r, s)| pkg2::formato::numero(s, i64::MAX as u64).map(|s| (r.to_string(), s)))
        .collect()
}

pub fn guardar_series(d: &mut dyn Disco, nivel: &Nivel, vistas: &[(String, u64)], nuevas: &[(String, u64)]) -> Result<(), Fallo> {
    let mut todas: BTreeMap<String, u64> = vistas.iter().cloned().collect();
    for (r, s) in nuevas {
        let x = todas.entry(r.clone()).or_insert(0);
        *x = (*x).max(*s);
    }
    if todas.iter().all(|(r, s)| vistas.iter().any(|(r2, s2)| r == r2 && s == s2)) && todas.len() == vistas.len() {
        return Ok(());
    }
    let mut t = String::new();
    for (r, s) in &todas {
        t += &format!("{r} {s}\n");
    }
    let ruta = nivel.paq("Series");
    let _ = d.borrar(&ruta);
    d.guardar(&ruta, t.as_bytes()).map_err(|x| de_disco(x, &ruta))
}

/// Lee la lista de repositorios y las claves, y baja y comprueba el índice de cada uno: su firma (con las claves de ese
/// repositorio), que sea de ese repositorio y de esta arquitectura, que no sea más viejo que el último visto (`vistas`)
/// y que no haya caducado (si la hora es creíble). Un índice que no vale no se usa: queda en `malos`.
pub fn cargar_repos(d: &mut dyn Disco, vistas: &[(String, u64)]) -> Result<Repos, Fallo> {
    let mut rs = Repos::default();
    match leer_texto(d, config::REPOSITORIOS)? {
        Some(t) => {
            let (r, av) = config::leer_repos(&t);
            rs.repos = r;
            rs.avisos.extend(av);
        }
        None => rs.avisos.push(format!("No hay lista de repositorios ({})", config::REPOSITORIOS)),
    }
    for (f, cajon) in listar_o_nada(d, config::CLAVES)? {
        if cajon || rs.claves.len() >= config::MAX_CLAVES {
            continue;
        }
        let ruta = unir(config::CLAVES, &f);
        match leer_texto(d, &ruta) {
            Ok(Some(t)) => match config::leer_clave(&f, &t) {
                Ok(c) => rs.claves.push(c),
                Err(por) => rs.avisos.push(format!("La clave {f} no vale: {por}")),
            },
            _ => rs.avisos.push(format!("La clave {f} no se puede leer")),
        }
    }
    let hora = d.hora().filter(|h| *h >= texto::CREIBLE_DESDE);
    for r in rs.repos.clone() {
        match cargar_indice(d, &r, &rs.claves, vistas, hora) {
            Ok(f) => {
                rs.series.push((r.nombre.clone(), f.indice.serie));
                rs.fuentes.push(f);
            }
            Err(por) => rs.malos.push((r.nombre.clone(), por)),
        }
    }
    Ok(rs)
}

fn cargar_indice(d: &mut dyn Disco, r: &Repo, claves: &[Clave], vistas: &[(String, u64)], hora: Option<u64>) -> Result<Fuente, String> {
    if r.url.contains("://") {
        return Err(RED.into());
    }
    let base = unir(&r.url, ARQUITECTURA);
    let ruta = unir(&base, "index.gz");
    let datos = d.leer(&ruta, MAX_INDICE_GZ).map_err(|e| format!("no se puede leer su índice ({ruta}: error {e})"))?;
    let sig = d.leer(&format!("{ruta}.sig"), firma::MAX_SIG).map_err(|_| "su índice no tiene firma".to_string())?;
    let publicas = config::de_indice(claves, &r.nombre);
    if publicas.is_empty() {
        return Err("no hay ninguna clave de confianza para él".into());
    }
    firma::comprobar(firma::INDICE, &datos, &sig, &publicas).map_err(|m| match m {
        firma::Mala::SinClave => "su índice está firmado con una clave que no es de confianza".to_string(),
        _ => "la firma de su índice no vale".to_string(),
    })?;
    let texto = gz::descomprimir(&datos, MAX_INDICE).map_err(|_| "su índice está estropeado".to_string())?;
    let texto = core::str::from_utf8(&texto).map_err(|_| "su índice no es texto".to_string())?;
    let ix = indice::leer(texto)?;
    if ix.repo != r.nombre {
        return Err(format!("su índice dice que es de otro repositorio ({})", ix.repo));
    }
    if ix.arquitectura != ARQUITECTURA {
        return Err(format!("su índice es de otra arquitectura ({})", ix.arquitectura));
    }
    if let Some((_, vista)) = vistas.iter().find(|(x, _)| *x == r.nombre)
        && ix.serie < *vista
    {
        return Err(format!("su índice es más viejo que el último visto (serie {} < {vista})", ix.serie));
    }
    if let (Some(h), Some(c)) = (hora, ix.caduca)
        && h > c
    {
        return Err(format!("su índice caducó el {}", texto::fecha_corta(c)));
    }
    Ok(Fuente { repo: r.clone(), base, indice: ix })
}

// ---------------------------------------------------------------------------------------------------------------
// Planes.

fn cajon_de_install(install: &str) -> Option<&str> {
    let mut p = install.split_whitespace();
    match (p.next(), p.next(), p.next()) {
        (Some("drawer"), Some(c), None) if cajon_valido(c) => Some(c),
        _ => None,
    }
}

fn paquete_de_entrada(f: &Fuente, e: &Entrada, nivel: &Nivel) -> Paquete {
    let mut ficheros = Vec::new();
    if let Some(c) = cajon_de_install(&e.install) {
        ficheros.push(nivel.en(c));
    }
    for (n, _) in &e.asignaciones {
        ficheros.push(format!("assign:{}", n.trim_end_matches(':')));
    }
    Paquete {
        repo: f.repo.nombre.clone(),
        nombre: e.nombre.clone(),
        version: e.version.clone(),
        requires: e.requires.clone(),
        conflicts: e.conflicts.clone(),
        provides: e.provides.clone(),
        ficheros,
        tamano: e.tamano,
        tamano_instalado: e.instalado,
        volumen: nivel.raiz.clone(),
        declarado: e.declarado,
    }
}

fn paquete_de_guardada(g: &Guardada, nivel: &Nivel) -> Paquete {
    let mut ficheros = vec![nivel.en(&g.ficha.cajon)];
    for (n, _) in &g.manifiesto.assigns {
        ficheros.push(format!("assign:{}", n.trim_end_matches(':')));
    }
    Paquete {
        repo: g.ficha.repo.clone(),
        nombre: g.ficha.nombre.clone(),
        version: g.ficha.version.clone(),
        requires: g.manifiesto.requires.clone(),
        conflicts: g.manifiesto.conflicts.clone(),
        provides: g.manifiesto.provides.clone(),
        ficheros,
        tamano: 0,
        tamano_instalado: 0,
        volumen: nivel.raiz.clone(),
        declarado: true,
    }
}

/// Lo que dice el resolutor o el comprobador, en llano.
pub fn en_llano(x: &str) -> String {
    let p: Vec<&str> = x.split(' ').collect();
    match p.as_slice() {
        ["missing", "pkg", n, ..] => format!("no hay ningún paquete {n} en los repositorios"),
        ["missing", resto @ ..] => format!("falta {} y ningún paquete lo trae", resto.join(" ")),
        ["conflict", a, b, ..] => format!("{a} choca con {b}"),
        ["machine", resto @ ..] => format!("este ordenador no tiene lo que pide ({})", resto.join(" ")),
        ["ambiguous", n, a, b] => format!("{n} está en {a} y en {b} con la misma prioridad: elige uno ({n}@{a})"),
        ["too-complex"] => "el plan es demasiado complicado".into(),
        ["requires", n, resto @ ..] => format!("{n} necesita {}", resto.join(" ")),
        ["breaks", n, resto @ ..] => format!("{n} dejaría de funcionar: necesita {}", resto.join(" ")),
        ["file", r, a, b] => format!("{a} y {b} quieren el mismo sitio ({r})"),
        ["hold", n] => format!("{n} está retenido"),
        ["downgrade", n] => format!("{n} bajaría de versión"),
        ["repo-change", n] => format!("{n} cambiaría de repositorio"),
        ["protected", n, r] => format!("{n} taparía uno del repositorio protegido {r}"),
        ["space", v] => format!("no cabe en {v}"),
        _ => x.to_string(),
    }
}

fn caso_de(e: &Estado, rs: &Repos, maquina: &Maquina) -> Caso {
    let mut c = Caso { maquina: maquina.clone(), ..Default::default() };
    for f in &rs.fuentes {
        c.repos.push(resolutor::Repo { nombre: f.repo.nombre.clone(), prioridad: f.repo.prioridad, protegido: f.repo.protegido });
        for x in &f.indice.entradas {
            c.entradas.push(resolutor::Entrada {
                repo: f.repo.nombre.clone(),
                nombre: x.nombre.clone(),
                version: x.version.clone(),
                id: x.id.clone(),
                rank: x.rank,
                requires: if x.declarado { x.requires.clone() } else { Vec::new() },
                recommends: x.recommends.clone(),
                provides: x.provides.clone(),
                conflicts: x.conflicts.clone(),
                replaces: x.replaces.clone(),
                autor: x.autor.clone(),
                uploader: x.uploader.clone(),
            });
        }
    }
    for p in &e.generacion.paquetes {
        c.instalado.insert(
            p.nombre.clone(),
            resolutor::Instalado { version: p.version.clone(), repo: p.repo.clone(), motivo: p.motivo.palabra().into(), hold: p.hold },
        );
    }
    c
}

/// Lo que queda instalado: nombre → (versión, repositorio, motivo, retenido).
type Destino = BTreeMap<String, (Version, String, Motivo, bool)>;

fn destino_de(g: &Generacion) -> Destino {
    g.paquetes.iter().map(|p| (p.nombre.clone(), (p.version.clone(), p.repo.clone(), p.motivo, p.hold))).collect()
}

fn texto_ver(v: &Version, r: bool) -> String {
    let mut t = if v.epoch > 0 { format!("{}:{}", v.epoch, v.version) } else { v.version.clone() };
    if r {
        t += &format!("-{}", v.release);
    }
    t
}

/// Aplica a `destino` las líneas del resolutor; las de "se queda como está" van a `quedan`.
fn aplicar_lineas(c: &Caso, lineas: &[String], destino: &mut Destino, quedan: &mut Vec<String>) -> Result<(), Fallo> {
    for l in lineas {
        let p: Vec<&str> = l.split(' ').collect();
        match p.as_slice() {
            ["+", n, v, repo, resto @ ..] => {
                let id = resto.iter().find_map(|x| x.strip_prefix('#'));
                let motivo = if resto.last().is_some_and(|m| *m == "(explicit)") { Motivo::Pedido } else { Motivo::Dependencia };
                let e = c
                    .entradas
                    .iter()
                    .filter(|e| e.repo == *repo && e.nombre == *n && texto_ver(&e.version, e.version.release != 1) == *v)
                    .find(|e| id.is_none_or(|i| e.id == i))
                    .ok_or_else(|| Fallo::Plan(vec![format!("el resolutor pide {n} {v}, que no está")]))?;
                destino.insert(n.to_string(), (e.version.clone(), repo.to_string(), motivo, false));
            }
            ["^", n, _vieja, "->", nueva, repo] => {
                let mut es: Vec<_> = c
                    .entradas
                    .iter()
                    .filter(|e| e.repo == *repo && e.nombre == *n && (texto_ver(&e.version, true) == *nueva || texto_ver(&e.version, false) == *nueva))
                    .collect();
                es.sort_by_key(|e| e.rank);
                let e = es.first().ok_or_else(|| Fallo::Plan(vec![format!("el resolutor pide {n} {nueva}, que no está")]))?;
                let (motivo, hold) = destino.get(*n).map_or((Motivo::Dependencia, false), |x| (x.2, x.3));
                destino.insert(n.to_string(), (e.version.clone(), repo.to_string(), motivo, hold));
            }
            ["-", n] => {
                destino.remove(*n);
            }
            ["=", n, resto @ ..] => quedan.push(format!("{n} se queda como está: {}", en_llano(resto.join(" ").trim_matches(|c| c == '(' || c == ')')))),
            _ => return Err(Fallo::Plan(vec![format!("el resolutor dice algo que no se entiende: {l}")])),
        }
    }
    Ok(())
}

/// Los cambios para pasar de `antes` a `despues`, en orden (lo que se quita, lo que cambia, lo que se instala).
fn diferencia(antes: &Destino, despues: &Destino) -> Vec<Cambio> {
    let mut v = Vec::new();
    for (n, (ver, _, _, _)) in antes {
        if !despues.contains_key(n) {
            v.push(Cambio::Quitar { nombre: n.clone(), version: ver.clone() });
        }
    }
    for (n, (a, repo, _, _)) in despues {
        if let Some((de, repo_antes, ..)) = antes.get(n) {
            if de == a && repo == repo_antes {
                continue;
            }
            if a.comparar(de) == Ordering::Greater {
                v.push(Cambio::Actualizar { nombre: n.clone(), de: de.clone(), a: a.clone(), repo: repo.clone() });
            } else {
                v.push(Cambio::Volver { nombre: n.clone(), de: de.clone(), a: a.clone(), repo: repo.clone() });
            }
        }
    }
    for (n, (a, repo, motivo, _)) in despues {
        if !antes.contains_key(n) {
            v.push(Cambio::Instalar { nombre: n.clone(), version: a.clone(), repo: repo.clone(), motivo: *motivo });
        }
    }
    v
}

/// Un plan: los cambios y lo que conviene decir.
#[derive(Clone, Debug, Default)]
pub struct Propuesta {
    pub cambios: Vec<Cambio>,
    pub avisos: Vec<String>,
    /// Lo pedido por su nombre (puede bajar de versión, cambiar de repositorio o tapar un protegido).
    pub pedidos: Vec<String>,
}

/// Instalar `pedidos` (`nombre` o `nombre@repositorio`).
pub fn plan_instalar(e: &Estado, rs: &Repos, maquina: &Maquina, nivel: &Nivel, pedidos: &[String]) -> Result<Propuesta, Fallo> {
    let mut c = caso_de(e, rs, maquina);
    let antes = destino_de(&e.generacion);
    let mut despues = antes.clone();
    let mut p = Propuesta { pedidos: pedidos.to_vec(), ..Default::default() };
    for pedido in pedidos {
        let nombre = pedido.split('@').next().unwrap_or("");
        if let Some(i) = despues.get(nombre) {
            p.avisos.push(format!("{nombre} ya está instalado ({})", bonita(&i.0)));
            continue;
        }
        let lineas = resolutor::resolver(&c, &Orden::Instalar(pedido.clone())).map_err(|x| {
            if x.starts_with("missing pkg ") && x[12..] == *nombre { Fallo::NoEsta(nombre.to_string()) } else { Fallo::Plan(vec![en_llano(&x)]) }
        })?;
        aplicar_lineas(&c, &lineas, &mut despues, &mut p.avisos)?;
        c.instalado = despues
            .iter()
            .map(|(n, (v, r, m, h))| (n.clone(), resolutor::Instalado { version: v.clone(), repo: r.clone(), motivo: m.palabra().into(), hold: *h }))
            .collect();
    }
    p.cambios = diferencia(&antes, &despues);
    comprobar_plan(e, rs, nivel, maquina, &p.cambios, &p.pedidos, None)?;
    Ok(p)
}

/// Todo al día. Lo que no se puede subir sin romper otra cosa se queda y se dice por qué.
pub fn plan_actualizar(e: &Estado, rs: &Repos, maquina: &Maquina, nivel: &Nivel) -> Result<Propuesta, Fallo> {
    let c = caso_de(e, rs, maquina);
    let antes = destino_de(&e.generacion);
    let mut despues = antes.clone();
    let mut p = Propuesta::default();
    let lineas = resolutor::resolver(&c, &Orden::Actualizar).map_err(|x| Fallo::Plan(vec![en_llano(&x)]))?;
    aplicar_lineas(&c, &lineas, &mut despues, &mut p.avisos)?;
    p.cambios = diferencia(&antes, &despues);
    comprobar_plan(e, rs, nivel, maquina, &p.cambios, &p.pedidos, None)?;
    Ok(p)
}

/// Si `x` (lo que queda) cumple la opción `o`.
fn cumple(o: &Opcion, x: &Paquete) -> bool {
    match o {
        Opcion::Req(Req::Pkg { nombre, .. }) => {
            let n = nombre.split('@').next().unwrap_or("");
            x.nombre == n || x.provides.iter().any(|p| matches!(p, Opcion::Req(Req::Pkg { nombre: pn, .. }) if pn == n))
        }
        Opcion::Req(Req::Libreria { nombre, .. }) => {
            x.provides.iter().any(|p| matches!(p, Opcion::Req(Req::Libreria { nombre: pn, .. }) if pn.eq_ignore_ascii_case(nombre)))
        }
        _ => false,
    }
}

/// Quitar `nombres`, y lo que solo estaba por ellos (lo traído como dependencia que ya no necesita nadie).
pub fn plan_quitar(e: &Estado, nivel: &Nivel, nombres: &[String]) -> Result<Propuesta, Fallo> {
    let antes = destino_de(&e.generacion);
    let mut despues = antes.clone();
    for n in nombres {
        if despues.remove(n.as_str()).is_none() {
            return Err(Fallo::Plan(vec![format!("{n} no está instalado en {}", nivel.raiz)]));
        }
    }
    // Lo que se alcanza desde lo pedido, siguiendo Requires y Recommends.
    let paquetes: BTreeMap<String, Paquete> = despues
        .iter()
        .filter_map(|(n, (v, ..))| e.guardada(n, v).map(|g| (n.clone(), paquete_de_guardada(g, nivel))))
        .collect();
    let mut vivos: Vec<String> = despues.iter().filter(|(_, x)| x.2 == Motivo::Pedido || x.3).map(|(n, _)| n.clone()).collect();
    let mut i = 0;
    while i < vivos.len() {
        let n = vivos[i].clone();
        i += 1;
        let Some(g) = e.guardada(&n, &despues[&n].0) else { continue };
        for o in g.manifiesto.requires.iter().chain(&g.manifiesto.recommends).flatten() {
            for (m, x) in &paquetes {
                if !vivos.contains(m) && cumple(o, x) {
                    vivos.push(m.clone());
                }
            }
        }
    }
    despues.retain(|n, _| vivos.contains(n));
    let p = Propuesta { cambios: diferencia(&antes, &despues), avisos: Vec::new(), pedidos: Vec::new() };
    comprobar_plan(e, &Repos::default(), nivel, &Maquina::default(), &p.cambios, &[], None)?;
    Ok(p)
}

/// Volver a la generación `a` (o, sin número, a la de antes de la que está en uso): lo que tenía ella.
pub fn plan_volver(e: &Estado, nivel: &Nivel, a: Option<u32>) -> Result<(Propuesta, u32), Fallo> {
    let actual = e.generacion.numero;
    let g = match a {
        Some(n) => e.generaciones.iter().find(|g| g.numero == n).ok_or_else(|| Fallo::Plan(vec![format!("no hay ninguna generación {n} (mira Paq Generaciones)")]))?,
        None => e.generaciones.iter().rev().find(|g| g.numero < actual).ok_or_else(|| Fallo::Plan(vec!["no hay ninguna generación anterior".into()]))?,
    };
    if g.numero == actual {
        return Err(Fallo::Plan(vec![format!("la generación {actual} es la que está en uso")]));
    }
    for p in &g.paquetes {
        if e.guardada(&p.nombre, &p.version).is_none() {
            return Err(Fallo::Danado(format!("{} {} ya no está en el almacén", p.nombre, bonita(&p.version))));
        }
    }
    let p = Propuesta { cambios: diferencia(&destino_de(&e.generacion), &destino_de(g)), avisos: Vec::new(), pedidos: g.paquetes.iter().map(|p| p.nombre.clone()).collect() };
    comprobar_plan(e, &Repos::default(), nivel, &Maquina::default(), &p.cambios, &p.pedidos, None)?;
    Ok((p, g.numero))
}

/// Lo que queda tras `cambios`, comprobado contra lo que hay: cada cambio parte de lo instalado ahora (si no, el plan es
/// viejo), y lo que pone está en el almacén o en un índice que vale.
fn destino_tras(e: &Estado, cambios: &[Cambio]) -> Result<Destino, Fallo> {
    let mut d = destino_de(&e.generacion);
    for c in cambios {
        let n = c.nombre().to_string();
        let ahora = d.get(&n).map(|x| x.0.clone());
        let viejo = || Fallo::PlanViejo(format!("{n}"));
        match c {
            Cambio::Instalar { version, repo, motivo, .. } => {
                if ahora.is_some() {
                    return Err(viejo());
                }
                d.insert(n, (version.clone(), repo.clone(), *motivo, false));
            }
            Cambio::Actualizar { de, a, repo, .. } | Cambio::Volver { de, a, repo, .. } => {
                if ahora.as_ref() != Some(de) {
                    return Err(viejo());
                }
                let x = d.get_mut(&n).unwrap();
                x.0 = a.clone();
                x.1 = repo.clone();
            }
            Cambio::Quitar { version, .. } => {
                if ahora.as_ref() != Some(version) {
                    return Err(viejo());
                }
                d.remove(&n);
            }
        }
    }
    Ok(d)
}

/// Comprueba un plan con `pkg2::plan::comprobar` (todo plan pasa por él antes de aplicarse): que lo que queda tiene lo
/// que necesita, que nada choca ni comparte cajón o asignación, que lo retenido no cambia, que nada baja de versión ni
/// cambia de repositorio sin pedirlo, y que cabe (`libre`, si se sabe).
fn comprobar_plan(e: &Estado, rs: &Repos, nivel: &Nivel, maquina: &Maquina, cambios: &[Cambio], pedidos: &[String], libre: Option<u64>) -> Result<(), Fallo> {
    if cambios.len() > MAX_CAMBIOS {
        return Err(Fallo::Plan(vec![format!("son {} cambios y caben {} en una confirmación: hazlo en dos veces", cambios.len(), MAX_CAMBIOS)]));
    }
    destino_tras(e, cambios)?;
    let mut estado = plan::Estado { maquina: maquina.clone(), ..Default::default() };
    for p in &e.generacion.paquetes {
        let g = e.guardada(&p.nombre, &p.version).ok_or_else(|| Fallo::Danado(format!("falta {} {} en el almacén", p.nombre, bonita(&p.version))))?;
        estado.instalados.push(plan::Instalado { paquete: paquete_de_guardada(g, nivel), hold: p.hold });
    }
    for f in rs.fuentes.iter().filter(|f| f.repo.protegido) {
        for x in &f.indice.entradas {
            estado.protegidos.push((f.repo.nombre.clone(), x.nombre.clone()));
        }
    }
    if let Some(l) = libre {
        estado.libre.push((nivel.raiz.clone(), l));
    }
    let mut pl = Plan { pasos: Vec::new(), pedidos: pedidos.to_vec() };
    for c in cambios {
        match c.queda() {
            None => pl.pasos.push(Paso::Quitar(c.nombre().to_string())),
            Some((v, repo)) => {
                let paquete = match e.guardada(c.nombre(), v) {
                    Some(g) => paquete_de_guardada(g, nivel),
                    None => {
                        let (f, x) = rs.entrada(c.nombre(), v, repo).ok_or_else(|| Fallo::NoEsta(format!("{} {} (de {repo})", c.nombre(), bonita(v))))?;
                        paquete_de_entrada(f, x, nivel)
                    }
                };
                pl.pasos.push(Paso::Poner(paquete));
            }
        }
    }
    let errores = plan::comprobar(&estado, &pl);
    if errores.is_empty() { Ok(()) } else { Err(Fallo::Plan(errores.iter().map(|x| en_llano(x)).collect())) }
}

// ---------------------------------------------------------------------------------------------------------------
// Aplicar.

/// Si `ruta` (relativa a donde se instala, como `Run` y `Assigns` en el formato 2) es el cajón `cajon` o algo de dentro.
fn dentro_de(ruta: &str, cajon: &str) -> bool {
    tar::ruta_valida(ruta) && (ruta == cajon || ruta.strip_prefix(cajon).is_some_and(|r| r.starts_with('/')))
}

/// La ruta en el disco de la dirección `url` de un paquete de `f`: relativa a `<url del repositorio>/<arquitectura>` o,
/// entera, dentro de ahí (como las escribe la referencia, `repositorio.py`). Si no, por qué no: es de la red, o no es
/// del repositorio (otro esquema, otro volumen, una ruta que se sale).
fn ruta_de_copia(f: &Fuente, url: &str) -> Result<String, &'static str> {
    let rel = match url.get(..f.base.len()) {
        Some(p) if p.eq_ignore_ascii_case(&f.base) && url[f.base.len()..].starts_with('/') => &url[f.base.len() + 1..],
        _ => url,
    };
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return Err(RED);
    }
    // `tar::ruta_valida` no deja `:` (ni esquemas ni volúmenes), `..`, `.`, partes vacías ni bytes de control.
    if !tar::ruta_valida(rel) {
        return Err("se sale del repositorio");
    }
    Ok(unir(&f.base, rel))
}

/// Lo que se dice de un repositorio o de una dirección de la red (el mismo texto: se guarda una vez en `C:Paq`).
const RED: &str = "es de la red, y Paq aún solo lee repositorios en un disco";

/// Prueba una dirección de `x`: la lee (sin pasar de su `Size` ni de `MAX_PAQUETE`) y mira su tamaño y su SHA-256. `Err` con por qué no
/// vale y si es que dio otra cosa (`true`: otro tamaño, más grande incluido, u otra suma) o que no se pudo bajar.
fn probar_copia(d: &mut dyn Disco, f: &Fuente, x: &Entrada, url: &str) -> Result<Vec<u8>, (&'static str, bool)> {
    let ruta = ruta_de_copia(f, url).map_err(|por| (por, false))?;
    // Nunca más de `MAX_PAQUETE`: con un `Size` mayor, lo leído no lo alcanza y es "otro tamaño".
    match d.leer(&ruta, (x.tamano as usize).min(MAX_PAQUETE)) {
        Ok(b) if b.len() as u64 != x.tamano => Err(("da otro tamaño", true)),
        Ok(b) if huella::sha256(&b) != x.sha256 => Err(("da otra suma", true)),
        Ok(b) => Ok(b),
        Err(disco::LLENO) => Err(("da otro tamaño", true)),
        Err(_) => Err(("no se puede leer", false)),
    }
}

/// Baja el paquete de `x` probando sus direcciones en orden (formato 2: el original y sus copias) hasta que una da el
/// `Size` y el `Sha256` del índice; lo que da otra cosa se tira y se prueba la siguiente. Si ninguna vale: `Huella` si
/// alguna dio un paquete cambiado (con lo que dio cada una, si son varias); si no, `Paquete` (no se pudo bajar).
///
/// Los textos, cortos a propósito: `C:Paq` va justo de páginas y el disco de pruebas, de sitio (bitácora `paq-formato`).
#[inline(never)]
fn bajar(d: &mut dyn Disco, f: &Fuente, x: &Entrada) -> Result<Vec<u8>, Fallo> {
    let mut lista = String::new();
    let mut alguna_mala = false;
    for url in &x.urls {
        match probar_copia(d, f, x, url) {
            Ok(b) => return Ok(b),
            Err((por, mala)) => {
                alguna_mala |= mala;
                lista += &format!("{}{url}: {por}", if lista.is_empty() { "" } else { "; " });
            }
        }
    }
    Err(if !alguna_mala {
        Fallo::Paquete(x.nombre.clone(), lista)
    } else if x.urls.len() == 1 {
        Fallo::Huella(format!("{} {}", x.nombre, bonita(&x.version)))
    } else {
        Fallo::Huella(format!("{} {} ({lista})", x.nombre, bonita(&x.version)))
    })
}

/// Baja un paquete, lo comprueba entero y lo deja en el almacén (sin tocar nada de lo que se ve).
fn traer(d: &mut dyn Disco, nivel: &Nivel, f: &Fuente, x: &Entrada) -> Result<Guardada, Fallo> {
    let n = x.nombre.clone();
    let malo = |por: String| Fallo::Paquete(n.clone(), por);
    let datos = bajar(d, f, x)?;
    let tope = if x.instalado > 0 { (x.instalado as usize).saturating_add(1 << 20).min(MAX_PAQUETE) } else { MAX_PAQUETE };
    let t = gz::descomprimir(&datos, tope.saturating_mul(2).min(MAX_PAQUETE)).map_err(|_| malo("su gzip está estropeado o es demasiado grande".into()))?;
    let es = tar::leer(&t, tope as u64).map_err(|m| malo(m.to_string()))?;
    let readmes: Vec<&tar::Entrada> = es.iter().filter(|x| !x.cajon && !x.ruta.contains('/') && x.ruta.to_ascii_lowercase().ends_with(".readme")).collect();
    let [leeme] = readmes.as_slice() else { return Err(malo("no lleva su manifiesto (un .readme en la raíz)".into())) };
    let readme = core::str::from_utf8(&t[leeme.datos.clone()]).map_err(|_| malo("su manifiesto no es UTF-8".into()))?.to_string();
    let m = pkg2::formato::leer(&readme, &leeme.ruta).map_err(|errs| malo(format!("su manifiesto no vale ({})", errs.join("; "))))?;
    if m.formato < 2 {
        return Err(malo("su manifiesto no es del formato 2 (Format: 2)".into()));
    }
    if m.nombre != x.nombre || m.version.as_ref() != Some(&x.version) {
        return Err(malo("su manifiesto dice otro nombre u otra versión que el índice".into()));
    }
    if !m.arquitecturas.iter().any(|a| a == ARQUITECTURA || a == "generic") {
        return Err(malo(format!("no es para {ARQUITECTURA}")));
    }
    let install = m.install.trim();
    if install.starts_with("system") || install.starts_with("installer") {
        return Err(Fallo::AunNo(format!("instalar paquetes \"{install}\" (solo \"drawer\")")));
    }
    let cajon = cajon_de_install(install).ok_or_else(|| malo(format!("no dice en qué cajón va (Install: drawer Nombre), o el cajón no vale: \"{install}\"")))?;
    if cajon_de_install(&x.install) != Some(cajon) {
        return Err(malo("su cajón no es el que dice el índice".into()));
    }
    if !m.variantes.is_empty() {
        return Err(Fallo::AunNo("instalar paquetes con variantes (Variant:)".into()));
    }
    for (a, c) in &m.assigns {
        let a = a.trim_end_matches(':');
        if !asignacion_valida(a) {
            return Err(malo(format!("declara una asignación que no puede ser suya ({a}:)")));
        }
        if !dentro_de(c, cajon) {
            return Err(malo(format!("su asignación {a}: no lleva a su cajón ({c})")));
        }
    }
    for s in &m.startup {
        let ok = s.strip_prefix("Run ").is_some_and(|r| dentro_de(r.trim(), cajon)) && m.permisos.iter().any(|p| p == "autostart");
        if !ok {
            return Err(malo(format!("su línea de arranque no es \"Run <su programa>\" con el permiso autostart: {s}")));
        }
    }
    let prefijo = format!("{cajon}/");
    let mut total = 0u64;
    for en in es.iter().filter(|en| en.ruta != leeme.ruta) {
        if en.ruta != cajon && !en.ruta.starts_with(&prefijo) {
            return Err(malo(format!("{} está fuera de su cajón ({cajon})", en.ruta)));
        }
        total += en.datos.len() as u64;
    }
    if x.instalado > 0 && total > x.instalado {
        return Err(malo("ocupa más de lo que dice el índice".into()));
    }
    // Al almacén, en un cajón que aún no cuenta (`.nuevo`); entero, se cambia de nombre.
    let h = hueco(&x.version);
    let dir_paquete = nivel.almacen(&n);
    hacer(d, &dir_paquete)?;
    let nuevo = unir(&dir_paquete, &format!("{h}.nuevo"));
    borrar_todo(d, &nuevo).map_err(|e| de_disco(e, &nuevo))?;
    hacer(d, &nuevo)?;
    let mut ficheros = Vec::new();
    for en in es.iter().filter(|en| en.ruta != leeme.ruta && en.ruta != cajon) {
        let rel = &en.ruta[prefijo.len()..];
        // Los cajones de por medio, si el tar no los trae antes.
        let partes: Vec<&str> = rel.split('/').collect();
        let mut camino = nuevo.clone();
        for p in &partes[..partes.len() - 1] {
            camino = unir(&camino, p);
            hacer(d, &camino)?;
        }
        let r = unir(&nuevo, rel);
        if en.cajon {
            hacer(d, &r)?;
        } else {
            let b = &t[en.datos.clone()];
            d.escribir(&r, b, en.modo).map_err(|e| de_disco(e, &r))?;
            ficheros.push(Fichero { crc: huella::crc32(b), tamano: b.len() as u64, ruta: en.ruta.clone() });
        }
    }
    let final_ = unir(&dir_paquete, &h);
    d.renombrar(&nuevo, &final_).map_err(|e| de_disco(e, &final_))?;
    let ficha = Ficha {
        nombre: n.clone(),
        version: x.version.clone(),
        repo: f.repo.nombre.clone(),
        id: x.id.clone(),
        sha256: x.sha256,
        fecha: d.hora().filter(|h| *h >= texto::CREIBLE_DESDE),
        cajon: cajon.to_string(),
        manifiesto: texto::suma(&readme),
        ficheros,
    };
    let rr = unir(&dir_paquete, &format!("{h}.readme"));
    d.guardar(&rr, readme.as_bytes()).map_err(|e| de_disco(e, &rr))?;
    let rp = unir(&dir_paquete, &format!("{h}.pkg"));
    d.guardar(&rp, ficha.escribir().as_bytes()).map_err(|e| de_disco(e, &rp))?;
    Ok(Guardada { ficha, manifiesto: m, readme, en_uso: false })
}

/// Hace la transacción `cambios` en `nivel` (que ya tiene que estar en orden y con el cerrojo de quien la hace), con lo
/// que dice `que` en la generación nueva. Devuelve su número.
pub fn aplicar(d: &mut dyn Disco, nivel: &Nivel, e: &mut Estado, rs: &Repos, maquina: &Maquina, cambios: &[Cambio], pedidos: &[String], que: &str) -> Result<u32, Fallo> {
    if cambios.is_empty() {
        return Err(Fallo::Plan(vec!["no hay nada que cambiar".into()]));
    }
    let libre = d.libre(&nivel.raiz);
    comprobar_plan(e, rs, nivel, maquina, cambios, pedidos, libre)?;
    let destino = destino_tras(e, cambios)?;
    // Lo que falta, al almacén: bajado, comprobado y guardado antes de tocar nada de lo que se ve.
    for c in cambios {
        let Some((v, repo)) = c.queda() else { continue };
        let desde_indice = rs.entrada(c.nombre(), v, repo);
        if let Some(g) = e.guardada(c.nombre(), v) {
            // Un paquete del repositorio con la misma versión y otra huella: cambiado en su sitio (formato 2: eso
            // entra con el release siguiente, nunca como el mismo).
            if let Some((_, x)) = desde_indice
                && x.sha256 != g.ficha.sha256
                && g.ficha.repo == repo
            {
                return Err(Fallo::Huella(format!("{} {} (el repositorio lo cambió sin cambiar su versión)", c.nombre(), bonita(v))));
            }
            continue;
        }
        let (f, x) = desde_indice.ok_or_else(|| Fallo::NoEsta(format!("{} {} (de {repo})", c.nombre(), bonita(v))))?;
        let g = traer(d, nivel, f, x)?;
        e.almacen.push(g);
    }
    // El cambio: la generación siguiente, de una vez.
    let numero = e.ultima + 1;
    let g = Generacion {
        numero,
        fecha: d.hora().filter(|h| *h >= texto::CREIBLE_DESDE),
        cambio: que.to_string(),
        paquetes: destino.into_iter().map(|(nombre, (version, repo, motivo, hold))| Instalado { nombre, version, repo, motivo, hold }).collect(),
    };
    let ruta = nivel.generacion(numero);
    d.guardar(&ruta, g.escribir().as_bytes()).map_err(|x| de_disco(x, &ruta))?;
    e.ultima = numero;
    e.generacion = g.clone();
    e.generaciones.push(g);
    reconciliar(d, nivel, e)?;
    Ok(numero)
}

// ---------------------------------------------------------------------------------------------------------------
// Limpiar.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conservar {
    /// La limpieza sola (S8): las tres últimas y las de los últimos 30 días.
    Solo,
    /// La de ahora y la de antes (para poder deshacer la última).
    Anterior,
    /// Solo la de ahora.
    Actual,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limpieza {
    pub generaciones: usize,
    pub versiones: usize,
    pub bytes: u64,
}

/// Borra las generaciones que no se guardan y las versiones del almacén que ya no usa ninguna.
pub fn limpiar(d: &mut dyn Disco, nivel: &Nivel, e: &mut Estado, que: Conservar) -> Result<Limpieza, Fallo> {
    let mut l = Limpieza::default();
    let actual = e.generacion.numero;
    let hora = d.hora().filter(|h| *h >= texto::CREIBLE_DESDE);
    let n = e.generaciones.len();
    let guarda = |i: usize, g: &Generacion| -> bool {
        g.numero == actual
            || match que {
                Conservar::Actual => false,
                Conservar::Anterior => i + 2 == n && g.numero < actual,
                Conservar::Solo => i + GUARDAR_ULTIMAS >= n || matches!((hora, g.fecha), (Some(h), Some(f)) if h.saturating_sub(f) < GUARDAR_DIAS * 86_400),
            }
    };
    let quedan: Vec<Generacion> = e.generaciones.iter().enumerate().filter(|(i, g)| guarda(*i, g)).map(|(_, g)| g.clone()).collect();
    let fuera: Vec<u32> = e.generaciones.iter().map(|g| g.numero).filter(|x| !quedan.iter().any(|g| g.numero == *x)).chain(e.danadas.iter().copied()).collect();
    for x in fuera {
        let r = nivel.generacion(x);
        match d.borrar(&r) {
            Ok(()) => l.generaciones += 1,
            Err(NO_EXISTE) => {}
            Err(y) => return Err(de_disco(y, &r)),
        }
    }
    e.generaciones = quedan;
    e.danadas.clear();
    let usada = |g: &Guardada| e.generaciones.iter().any(|x| x.paquetes.iter().any(|p| p.nombre == g.ficha.nombre && p.version == g.ficha.version));
    let sobran: Vec<usize> = (0..e.almacen.len()).filter(|&i| !e.almacen[i].en_uso && !usada(&e.almacen[i])).collect();
    for &i in sobran.iter().rev() {
        let g = e.almacen.remove(i);
        let base = nivel.hueco(&g.ficha.nombre, &g.ficha.version);
        // La ficha primero: un cajón sin ficha es basura que se barre; una ficha sin cajón sería "a la vista".
        for r in [format!("{base}.pkg"), format!("{base}.readme")] {
            match d.borrar(&r) {
                Ok(()) | Err(NO_EXISTE) => {}
                Err(y) => return Err(de_disco(y, &r)),
            }
        }
        borrar_todo(d, &base).map_err(|y| de_disco(y, &base))?;
        l.versiones += 1;
        l.bytes += g.ficha.ficheros.iter().map(|f| f.tamano).sum::<u64>();
        // Si no le queda ninguna versión, su cajón del almacén tampoco hace falta.
        if !e.almacen.iter().any(|x| x.ficha.nombre == g.ficha.nombre) {
            let _ = d.borrar(&nivel.almacen(&g.ficha.nombre));
        }
    }
    Ok(l)
}

#[cfg(test)]
#[path = "pruebas_motor.rs"]
pub(crate) mod pruebas;
