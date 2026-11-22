//! `Ajuste`: lee y pone un ajuste suelto (S36, regla 1; S27, apartado 7; `docs/bitacoras/ajustes-texto.md`).
//!
//! - `Ajuste LISTA` (o sin nada): los ajustes que conoce, con su valor y su fichero.
//! - `Ajuste <nombre>`: su valor (el de `ENV:`, el que está en uso; si no está, el guardado de `ENVARC:`).
//! - `Ajuste <nombre> <valor>`: lo pone en `ENV:` (se ve ya; se pierde al salir). Con `GUARDAR`, también en `ENVARC:`
//!   (la capa del usuario, `Home:Prefs/Env-Archive`), para siempre. Un valor con espacios, entre comillas.
//!
//! Cada ajuste es una clave de un fichero de texto de `ENV:`/`ENVARC:` (el formato de `idioma::texto`): la orden, la
//! ventana de Ajustes y una persona con un editor escriben el mismo fichero, y solo cambia su línea. Además de los que
//! conoce, vale cualquier `fichero.clave` (`Ajuste Escritorio/barra.segundos sí`), sin más comprobación que la del
//! formato.
//!
//! - `Ajuste DMS <settings.json>`: importa las barras de DankMaterialShell (`barConfigs`) a `ENV:Escritorio/barra`
//!   (con `GUARDAR`, también a `ENVARC:`), sobre lo que hubiera, y dice lo que DashOS no tiene y se dejó fuera
//!   (`docs/bitacoras/dms-barra.md`). `SYS:Prefs/Presets/DMS/shiva.json` es la barra del escritorio del usuario.
//!
//! Lo que es de la máquina (la zona horaria) o de seguridad (permisos, cuentas) no se cambia por aquí sin la
//! confirmación de `guardian`, que esta orden aún no sabe pedir: se niega y lo dice. Leerlo, sí.

use alloc::string::String;
use alloc::vec::Vec;
use crate::{di, Orden};
use dashos::protocolo::archivos as a;
use kit::idioma::prefs::{self, Prefs};
use kit::idioma::texto::{self, Ajustes};
use kit::idioma::Pais;

// Las barras de DMS: el modelo y el importador, los mismos que usan la barra y el centro.
#[path = "../../../usuario/src/barra_config.rs"]
#[allow(dead_code)]
mod barra_config;

/// Lo que se comprueba de un valor.
#[derive(Clone, Copy)]
enum Clase {
    /// Una lista de idiomas (`prefs::lista_de_idiomas`).
    Idiomas,
    /// Un país con su `LOCALE:Countries/<país>.country`.
    Pais,
    /// Uno de estos.
    Uno(&'static [&'static str]),
    /// Un color de acento del tema (`kit::tema::ACENTOS`) o `fondo` (la paleta del fondo de escritorio).
    Acento,
    /// De la máquina, en ese fichero (una línea): se lee, no se pone.
    Maquina(&'static str),
}

struct Conocido {
    nombre: &'static str,
    /// El fichero, desde `ENV:` o `ENVARC:`.
    fichero: &'static str,
    clave: &'static str,
    explica: &'static str,
    clase: Clase,
}

const CONOCIDOS: &[Conocido] = &[
    Conocido { nombre: "idioma", fichero: "Sys/locale", clave: prefs::CLAVE_IDIOMAS, explica: "los idiomas, por orden (english, español)", clase: Clase::Idiomas },
    Conocido { nombre: "pais", fichero: "Sys/locale", clave: prefs::CLAVE_PAIS, explica: "el país: fechas, números y dinero", clase: Clase::Pais },
    Conocido { nombre: "tema", fichero: "Escritorio/apariencia", clave: "modo", explica: "claro u oscuro", clase: Clase::Uno(&["claro", "oscuro"]) },
    Conocido { nombre: "acento", fichero: "Escritorio/apariencia", clave: "acento", explica: "el color: violeta, azul, verde, naranja, monocromo o fondo", clase: Clase::Acento },
    // S10: dónde van los menús de los programas (`docs/bitacoras/menus.md`); lo aplica el centro en marcha.
    Conocido { nombre: "menus", fichero: "Escritorio/menus", clave: "sitio", explica: "dónde van los menús: dentro (en cada ventana) o barra (arriba, como en el Amiga)", clase: Clase::Uno(&["dentro", "barra"]) },
    Conocido { nombre: "zona", fichero: "", clave: "", explica: "la zona horaria (de la máquina)", clase: Clase::Maquina(crate::ZONA) },
];

/// Lo que lleva encima un `Escritorio/apariencia` nuevo.
const CABECERA_TEMA: &str = "El tema (Ajustes > Apariencia; la orden `Ajuste tema` y `Ajuste acento`). Este fichero manda.\nmodo: claro u oscuro. acento: violeta, azul, verde, naranja, monocromo o fondo (la paleta del fondo de escritorio).";

/// Lo que lleva encima un `Escritorio/menus` nuevo (lo mismo que escribe el centro de control).
const CABECERA_MENUS: &str = "Dónde van los menús de los programas (Ajustes > Escritorio; la orden `Ajuste menus`). Este fichero manda.\nsitio: dentro (una fila en cada ventana) o barra (los del programa activo en la barra de arriba, como en el Amiga).";

/// Lo más que se lee de un fichero de ajustes (más, no se toca).
const MAX: usize = texto::MAX_FICHERO;

/// El ajuste `nombre`: uno conocido, o `fichero.clave`. `Err` con el porqué.
fn buscar(nombre: &str) -> Result<(String, String, Option<&'static Conocido>), String> {
    if let Some(c) = CONOCIDOS.iter().find(|c| c.nombre.eq_ignore_ascii_case(nombre)) {
        return Ok((c.fichero.into(), c.clave.into(), Some(c)));
    }
    let Some((fichero, clave)) = nombre.rsplit_once('.') else {
        return Err(alloc::format!("{}: no hay un ajuste con ese nombre (Ajuste LISTA los enseña; o fichero.clave)", nombre));
    };
    // El fichero, dentro de `ENV:`: sin volumen, sin subir y sin partes vacías.
    let parte_vale = |p: &str| !p.is_empty() && p != "." && p != ".." && !p.contains(':') && !p.chars().any(char::is_control);
    if !fichero.split('/').all(parte_vale) || !texto::clave_valida(clave) {
        return Err(alloc::format!("{}: no vale como fichero.clave (el fichero, dentro de ENV:; la clave, letras, cifras, _ y -)", nombre));
    }
    Ok((fichero.into(), clave.into(), None))
}

/// El fichero `camino` entero (por sus capas): `Ok(None)` si no está.
pub(crate) fn leer(o: &mut Orden, camino: &str) -> Result<Option<Vec<u8>>, i32> {
    match o.s.leer_entero(camino, MAX) {
        Ok(b) => Ok(Some(b)),
        Err(a::NO_EXISTE) => Ok(None),
        Err(e) => Err(e),
    }
}

/// El valor de `clave` en `asignacion:fichero`, y lo que se dice si no se entiende. `Ok(None)`: no está.
fn valor_en(o: &mut Orden, asignacion: &str, fichero: &str, clave: &str) -> Result<Option<String>, String> {
    let camino = alloc::format!("{}:{}", asignacion, fichero);
    match leer(o, &camino) {
        Ok(None) => Ok(None),
        Ok(Some(b)) => match Ajustes::leer(&b) {
            Ok(x) => Ok(x.valor(clave).map(String::from)),
            Err(texto::Error::NoEsTexto) => Err(alloc::format!("{}: no es texto", camino)),
            Err(texto::Error::Enorme) => Err(alloc::format!("{}: demasiado grande", camino)),
        },
        Err(a::SIN_SITIO) => Err(alloc::format!("{}: demasiado grande", camino)),
        Err(e) => Err(alloc::format!("{}: {}", camino, crate::texto_error(e))),
    }
}

/// La primera capa de una asignación (donde se escribe lo del usuario): `RAM:Env`, `Home:Prefs/Env-Archive`.
fn capa(o: &Orden, asignacion: &str) -> Option<String> {
    let l = o.s.tabla.buscar(asignacion.as_bytes())?;
    let primera = l.capas().next()?;
    core::str::from_utf8(primera).ok().map(String::from)
}

/// Escribe `bytes` en el fichero `fichero` de la primera capa de `asignacion`, creando los cajones que falten. El
/// camino escrito.
pub(crate) fn escribir(o: &mut Orden, asignacion: &str, fichero: &str, bytes: &[u8]) -> Result<String, String> {
    let base = capa(o, asignacion).ok_or_else(|| alloc::format!("{}: no está en la tabla de nombres", asignacion))?;
    let camino = alloc::format!("{}/{}", base, fichero);
    // Los cajones de delante.
    let partes: Vec<&str> = fichero.split('/').collect();
    let mut cajon = base.clone();
    for p in &partes[..partes.len() - 1] {
        cajon = alloc::format!("{}/{}", cajon, p);
        match o.s.mirar(&cajon) {
            Ok(d) if d.cajon() => {}
            Ok(_) => return Err(alloc::format!("{}: {}", cajon, crate::texto_error(a::NO_ES_CAJON))),
            Err(a::NO_EXISTE) => {
                let asa = o.s.crear(&cajon, a::CAJON).map_err(|e| alloc::format!("{}: {}", cajon, crate::texto_error(e)))?;
                o.s.cerrar(asa);
            }
            Err(e) => return Err(alloc::format!("{}: {}", cajon, crate::texto_error(e))),
        }
    }
    let (asa, _) = o.s.abrir_para_escribir(&camino, false).map_err(|e| alloc::format!("{}: {}", camino, crate::texto_error(e)))?;
    let r = o.s.escribir(asa, 0, bytes);
    let cerrado = o.s.cerrar(asa);
    r.map_err(|e| alloc::format!("{}: {}", camino, crate::texto_error(e)))?;
    if cerrado != a::BIEN {
        return Err(alloc::format!("{}: {}", camino, crate::texto_error(cerrado)));
    }
    Ok(camino)
}

/// Comprueba `valor` para el ajuste conocido `c`; devuelve el valor tal como se guarda.
fn comprobar(o: &mut Orden, c: &Conocido, valor: &str) -> Result<String, String> {
    match c.clase {
        Clase::Idiomas => {
            let v = prefs::lista_de_idiomas(valor).map_err(|x| alloc::format!("{:?} no vale como idioma (o son más de {})", x, prefs::MAX_IDIOMAS))?;
            // Uno que no tiene sus textos en `LOCALE:Languages` se pone igual (los catálogos de cada programa pueden
            // estar), pero se avisa.
            for i in v.iter().filter(|i| !i.eq_ignore_ascii_case(kit::idioma::IDIOMA_DE_SERIE)) {
                if o.s.mirar(&alloc::format!("LOCALE:Languages/{}.language", i)).is_err() {
                    di!(o, "Aviso: no hay LOCALE:Languages/{}.language: los días y los meses irán en el siguiente idioma\n", i);
                }
            }
            Ok(v.join(", "))
        }
        Clase::Pais => {
            let p = valor.trim();
            if p.is_empty() || !kit::idioma::letras::nombre_valido(p) {
                return Err(alloc::format!("{:?} no vale como país", p));
            }
            let hay = [String::from(p), p.to_lowercase()].iter().any(|n| o.s.mirar(&alloc::format!("LOCALE:Countries/{}.country", n)).is_ok());
            if !hay {
                return Err(alloc::format!("{}: no hay LOCALE:Countries/{}.country (List LOCALE:Countries enseña los que hay)", p, p));
            }
            Ok(p.into())
        }
        Clase::Uno(validos) => {
            let v = valor.trim();
            validos.iter().find(|x| x.eq_ignore_ascii_case(v)).map(|x| String::from(*x)).ok_or_else(|| alloc::format!("{:?} no vale: {}", v, validos.join(", ")))
        }
        Clase::Acento => {
            let v = valor.trim();
            if v.eq_ignore_ascii_case("fondo") {
                return Ok("fondo".into());
            }
            if v.eq_ignore_ascii_case("monocromo") {
                return Ok("monocromo".into());
            }
            kit::tema::ACENTOS
                .iter()
                .find(|x| x.0.eq_ignore_ascii_case(v))
                .map(|x| x.0.to_lowercase())
                .ok_or_else(|| alloc::format!("{:?} no vale: violeta, azul, verde, naranja, monocromo o fondo", v))
        }
        Clase::Maquina(_) => Err(String::new()),
    }
}

/// El IFF del Amiga que va con `Sys/locale` (S36: el texto manda y el `.prefs` se genera de él): los idiomas y el
/// país del texto, los datos del país de su `.country` y la diferencia con Greenwich de la zona del sistema (el Amiga
/// no sabe de cambio de hora: la de ahora).
fn locale_prefs(o: &mut Orden, texto: &[u8]) -> Option<Vec<u8>> {
    let mut p = Prefs::de_texto(texto).ok().flatten()?;
    if !p.pais.is_empty() {
        for n in [p.pais.clone(), p.pais.to_lowercase()] {
            if let Ok(b) = o.s.leer_entero(&alloc::format!("LOCALE:Countries/{}.country", n), 4096) {
                p.datos = Pais::leer(&b);
                break;
            }
        }
    }
    let ahora = dashos::hora_ns() / 1_000_000_000;
    p.gmt = o.s.zona().map_or(0, |z| -(dashos::zona::desfase_s(z, ahora) / 60) as i32);
    Some(p.escribir())
}

fn listar(o: &mut Orden) {
    di!(o, "Ajustes (Ajuste <nombre> <valor> lo pone en ENV:; con GUARDAR, también en ENVARC:):\n");
    for c in CONOCIDOS {
        let (valor, donde) = match c.clase {
            Clase::Maquina(camino) => (o.s.leer_entero(camino, 256).ok().and_then(|b| String::from_utf8(b).ok()).map(|t| String::from(t.lines().next().unwrap_or("").trim())), String::from(camino)),
            _ => {
                let v = valor_en(o, "ENV", c.fichero, c.clave).ok().flatten();
                (v, alloc::format!("ENV:{} {}", c.fichero, c.clave))
            }
        };
        let valor = valor.filter(|v| !v.is_empty()).unwrap_or_else(|| String::from("(de serie)"));
        di!(o, "  {:<8} {:<20} {} ({})\n", c.nombre, valor, c.explica, donde);
    }
    di!(o, "Y cualquier otro como fichero.clave, con el fichero dentro de ENV: (Escritorio/barra.segundos).\n");
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::AJUSTE);
    let nombre = args.texto("NOMBRE").map(String::from);
    let valor = args.texto("VALOR").map(String::from);
    let guardar = args.si("GUARDAR");
    if let Some(desde) = args.texto("DMS").map(String::from) {
        return importar_dms(o, &desde, guardar);
    }
    let Some(nombre) = nombre.filter(|_| !args.si("LISTA")) else {
        listar(o);
        return o.fin(shell::RC_BIEN);
    };
    let (fichero, clave, conocido) = match buscar(&nombre) {
        Ok(x) => x,
        Err(porque) => {
            o.error(format_args!("{}", porque));
            return o.fin(shell::RC_FALLO);
        }
    };
    // De la máquina: se lee de su fichero; ponerlo pide `guardian`.
    if let Some(Conocido { clase: Clase::Maquina(camino), .. }) = conocido {
        if valor.is_some() {
            o.error(format_args!(
                "{} es un ajuste de la máquina ({}): cambiarlo pide la confirmación de guardian, y esta orden aún no sabe pedirla. No se ha cambiado nada",
                nombre, camino
            ));
            return o.fin(shell::RC_FALLO);
        }
        return match o.s.leer_entero(camino, 256) {
            Ok(b) => {
                let t = String::from_utf8_lossy(&b).into_owned();
                di!(o, "{}: {} ({})\n", nombre, t.lines().next().unwrap_or("").trim(), camino);
                o.fin(shell::RC_BIEN)
            }
            Err(e) => {
                o.error_de(e, camino);
                o.fin(shell::RC_AVISO)
            }
        };
    }
    let Some(valor) = valor else {
        // Leer: el de `ENV:` y, si no está, el guardado.
        for asignacion in ["ENV", "ENVARC"] {
            match valor_en(o, asignacion, &fichero, &clave) {
                Ok(Some(v)) => {
                    di!(o, "{}: {} ({}:{} {})\n", nombre, v, asignacion, fichero, clave);
                    return o.fin(shell::RC_BIEN);
                }
                Ok(None) => {}
                Err(porque) => {
                    o.error(format_args!("{}; se usa lo de serie", porque));
                    return o.fin(shell::RC_AVISO);
                }
            }
        }
        di!(o, "{}: (de serie; no está en ENV:{} ni en ENVARC:{})\n", nombre, fichero, fichero);
        return o.fin(shell::RC_BIEN);
    };
    let valor = match conocido {
        Some(c) => match comprobar(o, c, &valor) {
            Ok(v) => v,
            Err(porque) => {
                o.error(format_args!("{}: {}. No se ha cambiado nada", nombre, porque));
                return o.fin(shell::RC_FALLO);
            }
        },
        None => valor.trim().into(),
    };
    // Primero se prepara todo (en `ENV:` y, con GUARDAR, en `ENVARC:`); si algo no vale, no se escribe nada.
    let mut por_escribir: Vec<(&str, Vec<u8>)> = Vec::new();
    for asignacion in if guardar { &["ENV", "ENVARC"][..] } else { &["ENV"][..] } {
        // Sobre lo que hay en esa asignación (en `ENV:`, la copia de cuando entró; en `ENVARC:`, la capa que se ve):
        // así sus comentarios y lo demás se quedan.
        let camino = alloc::format!("{}:{}", asignacion, fichero);
        let antes = match leer(o, &camino) {
            Ok(b) => b,
            Err(a::SIN_SITIO) => {
                o.error(format_args!("{} es demasiado grande: no se toca (arréglalo a mano o bórralo). No se ha cambiado nada", camino));
                return o.fin(shell::RC_FALLO);
            }
            Err(e) => {
                o.error(format_args!("{}: {}. No se ha cambiado nada", camino, crate::texto_error(e)));
                return o.fin(shell::RC_FALLO);
            }
        };
        let mut x = match antes.as_deref().map(Ajustes::leer) {
            None => {
                let mut x = Ajustes::nuevo();
                if fichero == "Sys/locale" {
                    x.comentar(prefs::CABECERA_TEXTO);
                } else if fichero == "Escritorio/apariencia" {
                    x.comentar(CABECERA_TEMA);
                } else if fichero == "Escritorio/menus" {
                    x.comentar(CABECERA_MENUS);
                }
                x
            }
            Some(Ok(x)) => x,
            // Uno roto no se pisa: lo escribió alguien, y no se sabe qué quería.
            Some(Err(e)) => {
                let porque = if e == texto::Error::NoEsTexto { "no es texto" } else { "es demasiado grande" };
                o.error(format_args!("{} {}: no se toca (arréglalo a mano o bórralo). No se ha cambiado nada", camino, porque));
                return o.fin(shell::RC_FALLO);
            }
        };
        if let Err(e) = x.poner(&clave, &valor) {
            let porque = match e {
                texto::MalValor::Clave => "la clave no vale",
                texto::MalValor::Valor => "el valor lleva saltos de línea u otros bytes de control, o es demasiado largo",
                texto::MalValor::Enorme => "el fichero se haría demasiado grande",
            };
            o.error(format_args!("{}: {}. No se ha cambiado nada", nombre, porque));
            return o.fin(shell::RC_FALLO);
        }
        por_escribir.push((asignacion, x.texto().into_bytes()));
    }
    let mut escritos = Vec::new();
    for (asignacion, bytes) in &por_escribir {
        match escribir(o, asignacion, &fichero, bytes) {
            Ok(c) => escritos.push(c),
            Err(porque) => {
                o.error(format_args!("{}", porque));
                return o.fin(shell::RC_FALLO);
            }
        }
        // El idioma y el país: el IFF del Amiga, generado del texto, al lado.
        if fichero == "Sys/locale"
            && let Some(iff) = locale_prefs(o, bytes)
            && let Err(porque) = escribir(o, asignacion, "Sys/locale.prefs", &iff)
        {
            o.error(format_args!("{} (el texto sí se escribió)", porque));
        }
    }
    di!(o, "{}: {} (en {})\n", nombre, valor, escritos.join(" y "));
    o.fin(shell::RC_BIEN)
}

/// `Ajuste DMS <settings.json> [GUARDAR]`: las barras de DMS a `ENV:Escritorio/barra` (y a `ENVARC:`), sobre lo que hubiera
/// (sus comentarios se quedan). Si algo no vale, no se escribe nada.
fn importar_dms(o: &mut Orden, desde: &str, guardar: bool) -> u64 {
    let bytes = match o.s.leer_entero(desde, 4 << 20) {
        Ok(b) => b,
        Err(e) => {
            o.error_de(e, desde);
            return o.fin(shell::RC_FALLO);
        }
    };
    let (cfg, dice) = match barra_config::importar_dms(&bytes) {
        Ok(x) => x,
        Err(porque) => {
            o.error(format_args!("{}: {}. No se ha cambiado nada", desde, porque));
            return o.fin(shell::RC_FALLO);
        }
    };
    let fichero = barra_config::FICHERO;
    let mut por_escribir: Vec<(&str, Vec<u8>)> = Vec::new();
    for asignacion in if guardar { &["ENV", "ENVARC"][..] } else { &["ENV"][..] } {
        let camino = alloc::format!("{}:{}", asignacion, fichero);
        let antes = match leer(o, &camino) {
            Ok(b) => b.filter(|b| Ajustes::leer(b).is_ok()),
            Err(e) => {
                o.error(format_args!("{}: {}. No se ha cambiado nada", camino, crate::texto_error(e)));
                return o.fin(shell::RC_FALLO);
            }
        };
        match cfg.escribir(antes.as_deref()) {
            Ok(b) => por_escribir.push((asignacion, b)),
            Err(porque) => {
                o.error(format_args!("{}: {}. No se ha cambiado nada", camino, porque));
                return o.fin(shell::RC_FALLO);
            }
        }
    }
    let mut escritos = Vec::new();
    for (asignacion, bytes) in &por_escribir {
        match escribir(o, asignacion, fichero, bytes) {
            Ok(c) => escritos.push(c),
            Err(porque) => {
                o.error(format_args!("{}", porque));
                return o.fin(shell::RC_FALLO);
            }
        }
    }
    // El color (DMS dinámico con scheme-monochrome: el monocromo), en `Escritorio/apariencia`, solo su clave.
    let mut dice = dice;
    if let Some((acento, que)) = barra_config::acento_dms(&bytes) {
        dice.push(que);
        if !acento.is_empty() {
            for asignacion in if guardar { &["ENV", "ENVARC"][..] } else { &["ENV"][..] } {
                let camino = alloc::format!("{}:Escritorio/apariencia", asignacion);
                let mut x = match leer(o, &camino) {
                    Ok(Some(b)) => match Ajustes::leer(&b) {
                        Ok(x) => x,
                        Err(_) => {
                            dice.push(alloc::format!("{} no se lee: el color no se toca", camino));
                            continue;
                        }
                    },
                    Ok(None) => {
                        let mut x = Ajustes::nuevo();
                        x.comentar(CABECERA_TEMA);
                        x
                    }
                    Err(e) => {
                        dice.push(alloc::format!("{}: {}: el color no se toca", camino, crate::texto_error(e)));
                        continue;
                    }
                };
                if x.poner("acento", acento).is_ok()
                    && let Err(porque) = escribir(o, asignacion, "Escritorio/apariencia", x.texto().as_bytes())
                {
                    dice.push(porque);
                }
            }
        }
    }
    let barras: Vec<String> = cfg.barras.iter().map(|b| alloc::format!("{} ({}, {} widgets)", b.id, b.posicion.nombre(), b.zonas.iter().map(Vec::len).sum::<usize>())).collect();
    di!(o, "Barras de DMS importadas de {}: {} (en {})\n", desde, barras.join(", "), escritos.join(" y "));
    if !dice.is_empty() {
        di!(o, "Lo que DashOS no tiene o cambió:\n");
        for d in &dice {
            di!(o, "  {}\n", d);
        }
    }
    o.fin(shell::RC_BIEN)
}
